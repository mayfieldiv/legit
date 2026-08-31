//! Local wayfinder Effort discovery and dialect parsing — the ticket
//! surface's one local-filesystem interface (spec §2.2 + §3), the sibling of
//! `github::wayfinder`. An Effort directory parses into an [`EffortRead`]:
//! fully normalized or visibly degraded, never silently partial. Which file
//! marks an Effort, the two ticket dialects, and the normalization rules are
//! all implementation.

// TODO(#120): remove once the fetch layer dispatches local probes.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::{
    canonical_path::CanonicalPathBuf,
    map_body::{first_h1, scan_map_body},
    ticket::{
        Claim, Dependency, Effort, EffortKey, EffortRead, ExternalDependency, Ticket, TicketKey,
        TicketState, TicketType,
    },
};

/// The directories a ticket file may live in, probed in this order in every
/// Effort: the older dialect's `tickets/`, the newer dialect's `issues/`.
const TICKET_DIRS: &[&str] = &["tickets", "issues"];

/// Parse one Effort directory (a directory holding a `map.md`) into an
/// [`EffortRead`]. Errs only when the directory itself can't be
/// canonicalized — without that there is no identity to degrade under;
/// every parse failure past that point degrades the Effort instead (spec
/// §5.5: never a crash, never silent).
pub fn read_effort(dir: &Path) -> anyhow::Result<EffortRead> {
    let dir = CanonicalPathBuf::canonicalize(dir)
        .with_context(|| format!("canonicalizing effort dir {}", dir.display()))?;
    let key = EffortKey::Local { dir: dir.clone() };
    // Map context is read first so a later ticket failure degrades with the
    // best available title/Destination rather than losing them.
    let (title, destination) = match read_map(&dir) {
        Ok(map) => map,
        Err(reason) => {
            return Ok(EffortRead::Degraded {
                key,
                title: dir_title(&dir),
                destination: None,
                reason,
            });
        }
    };
    let read = read_tickets(&dir).and_then(|tickets| {
        Effort::new(key.clone(), title.clone(), destination.clone(), tickets)
            .map_err(|error| format!("{error:#}"))
    });
    Ok(match read {
        Ok(effort) => EffortRead::Ready(effort),
        Err(reason) => EffortRead::Degraded {
            key,
            title,
            destination,
            reason,
        },
    })
}

/// Probe one Wayfinder Root for Effort directories. A root either *is* a
/// single Effort (a map file directly inside) or *contains* Effort
/// subdirectories — both shapes exist in the wild (spec §2.2). A missing or
/// effort-less root probes empty; an unreadable one errs, surfaced per repo
/// by discovery (§5.5).
pub fn probe_root(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    if find_map_file(root)
        .map_err(anyhow::Error::msg)?
        .is_some()
    {
        return Ok(vec![root.to_owned()]);
    }
    let entries =
        fs::read_dir(root).with_context(|| format!("reading root {}", root.display()))?;
    let mut efforts = Vec::new();
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() && find_map_file(&path).map_err(anyhow::Error::msg)?.is_some() {
            efforts.push(path);
        }
    }
    efforts.sort();
    Ok(efforts)
}

/// The Effort title of last resort: the effort directory's name.
fn dir_title(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// Read the Effort's map file: title (the H1, falling back to the directory
/// name — real maps may open with an HTML marker comment and no heading) and
/// Destination.
fn read_map(dir: &Path) -> Result<(String, Option<String>), String> {
    let path = find_map_file(dir)?.ok_or("no map.md in effort directory")?;
    let body = fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()))?;
    let title = first_h1(&body).unwrap_or_else(|| dir_title(dir));
    Ok((title, scan_map_body(&body).destination))
}

/// The map file marking `dir` as an Effort, matched case-insensitively
/// (`MAP.md` exists in the wild). Lexicographically first on the pathological
/// case of several case-variants coexisting, for determinism.
fn find_map_file(dir: &Path) -> Result<Option<PathBuf>, String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("reading {}: {error}", dir.display()))?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("map.md"))
        })
        .collect();
    candidates.sort();
    Ok(candidates.into_iter().next())
}

/// One ticket file found in the Effort's ticket directories, before parsing:
/// its identity plus the numeric filename prefix `blocked-by` refs resolve
/// against.
struct MemberFile {
    path: PathBuf,
    key: TicketKey,
    number: Option<u64>,
}

/// Read and normalize every ticket file in the Effort. Any per-file failure
/// degrades the whole Effort — a misparsed authoritative field could put a
/// Ticket falsely on the Frontier, and there is no conservative reading.
fn read_tickets(dir: &Path) -> Result<Vec<Ticket>, String> {
    let members = list_member_files(dir)?;
    members
        .iter()
        .map(|member| {
            parse_ticket_file(member, &members)
                .map_err(|reason| format!("{}: {reason}", member.path.display()))
        })
        .collect()
}

/// Enumerate the Effort's ticket files: `.md` files directly inside each
/// ticket directory, in filename order (effort order). Subdirectories —
/// `assets/`, ticket-scoped or not — are never tickets.
fn list_member_files(dir: &Path) -> Result<Vec<MemberFile>, String> {
    let mut members = Vec::new();
    for sub in TICKET_DIRS {
        let ticket_dir = dir.join(sub);
        if !ticket_dir.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&ticket_dir)
            .map_err(|error| format!("reading {}: {error}", ticket_dir.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        paths.sort();
        for path in paths {
            let key = TicketKey::Local {
                path: CanonicalPathBuf::canonicalize(&path)
                    .map_err(|error| format!("canonicalizing {}: {error}", path.display()))?,
            };
            members.push(MemberFile {
                number: filename_number(&path),
                path,
                key,
            });
        }
    }
    Ok(members)
}

/// The `NN` of an `NN-slug.md` filename — what a `blocked-by` ref names.
/// `None` for a file without the numeric prefix (it can't be a target).
fn filename_number(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    let digits: &str = &stem[..stem
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(stem.len())];
    (!digits.is_empty()).then(|| digits.parse().ok())?
}

/// The display slug of a ticket file (`01-inventory`) — the title of last
/// resort for a file with no H1.
fn file_slug(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Parse one ticket file into a [`Ticket`], resolving its `blocked-by` refs
/// against the Effort's member files.
fn parse_ticket_file(member: &MemberFile, members: &[MemberFile]) -> Result<Ticket, String> {
    let content = fs::read_to_string(&member.path).map_err(|error| format!("reading: {error}"))?;
    // Per-file dialect detection: frontmatter marks the older dialect,
    // anything else reads as the newer one.
    let (fields, body) = if content.starts_with("---\n") {
        parse_older_dialect(&content)?
    } else {
        (parse_newer_dialect(&content)?, content.as_str())
    };

    let (state, claim) = fields.lifecycle()?;
    let mut dependencies = Vec::new();
    for reference in fields.blocked_by {
        let number: u64 = reference
            .parse()
            .map_err(|_| format!("blocked-by ref {reference:?} is not a ticket number"))?;
        dependencies.push(match members.iter().find(|m| m.number == Some(number)) {
            Some(target) => Dependency::SameEffort(target.key.clone()),
            // No member file carries that number: the target can't be
            // found, and an unseen Dependency must never put the ticket
            // on the Frontier.
            None => Dependency::Unknown { raw: reference },
        });
    }
    for reference in fields.external_blocked_by {
        dependencies.push(resolve_external_ref(&member.path, &reference, members));
    }

    Ok(Ticket {
        key: member.key.clone(),
        title: first_h1(body).unwrap_or_else(|| file_slug(&member.path)),
        state,
        claim,
        ty: TicketType(fields.ty.unwrap_or_default()),
        dependencies,
    })
}

/// Resolve one `external-blocked-by` ref — a relative path from the ticket
/// file's directory (`../../<effort>/tickets/NN-slug.md` in the corpus). A
/// readable target parses for the state and title the edge carries; a path
/// that lands back inside this Effort folds to a SameEffort edge; anything
/// unresolvable or unreadable is an Unknown Dependency with the raw ref kept
/// for display — never an error, so one dead ref doesn't degrade the Effort
/// (the ticket it blocks stays off the Frontier either way).
fn resolve_external_ref(ticket_path: &Path, reference: &str, members: &[MemberFile]) -> Dependency {
    let unknown = || Dependency::Unknown {
        raw: reference.to_owned(),
    };
    let Some(base) = ticket_path.parent() else {
        return unknown();
    };
    let Ok(path) = CanonicalPathBuf::canonicalize(base.join(reference)) else {
        return unknown();
    };
    let key = TicketKey::Local { path: path.clone() };
    if let Some(member) = members.iter().find(|member| member.key == key) {
        return Dependency::SameEffort(member.key.clone());
    }
    let Ok(content) = fs::read_to_string(&path) else {
        return unknown();
    };
    let parsed = if content.starts_with("---\n") {
        parse_older_dialect(&content)
    } else {
        parse_newer_dialect(&content).map(|fields| (fields, content.as_str()))
    };
    let Ok((fields, body)) = parsed else {
        return unknown();
    };
    let Ok((state, _)) = fields.lifecycle() else {
        return unknown();
    };
    Dependency::External(ExternalDependency {
        key,
        state,
        title: first_h1(body),
    })
}

/// A ticket file's lifecycle-bearing fields, as either dialect spells them
/// (#106). Every field is optional; unknown keys are ignored.
#[derive(Default)]
struct TicketFields {
    /// Older dialect: `open`/`closed`. Newer dialect: `claimed`/`resolved` —
    /// its only lifecycle vocabulary, carrying the claim too.
    status: Option<String>,
    ty: Option<String>,
    /// Older dialect only. `Some("")` when the key is present with no value —
    /// unclaimed, but distinct from an absent key only in intent, not
    /// normalization.
    assignee: Option<String>,
    blocked_by: Vec<String>,
    external_blocked_by: Vec<String>,
}

impl TicketFields {
    /// Normalize the two lifecycle axes out of the dialect vocabularies. The
    /// status field is the only lifecycle authority (spec §3.2) — body prose
    /// (`## Closure`, superseded blockquotes, handoff notes) never is.
    fn lifecycle(&self) -> Result<(TicketState, Option<Claim>), String> {
        // Present-but-empty `assignee:` is unclaimed — observed in the wild.
        let assignee = self
            .assignee
            .clone()
            .filter(|assignee| !assignee.is_empty())
            .map(Claim::By);
        match self.status.as_deref() {
            None => Ok((TicketState::Open, assignee)),
            Some(value) if value.eq_ignore_ascii_case("open") => Ok((TicketState::Open, assignee)),
            Some(value) if value.eq_ignore_ascii_case("closed") => {
                Ok((TicketState::Closed, assignee))
            }
            // Newer dialect: claimed-ness without a claimant name.
            Some(value) if value.eq_ignore_ascii_case("claimed") => {
                Ok((TicketState::Open, Some(Claim::Anonymous)))
            }
            Some(value) if value.eq_ignore_ascii_case("resolved") => {
                Ok((TicketState::Closed, None))
            }
            Some(other) => Err(format!("unrecognized status {other:?}")),
        }
    }
}

/// Parse the newer dialect's prose field lines: `Status:`, `Type:`, and
/// `Blocked by: NN, NN` in the leading lines, before the first section
/// heading (`##` or deeper) — past it, matching text is body prose, never
/// lifecycle.
fn parse_newer_dialect(content: &str) -> Result<TicketFields, String> {
    let mut fields = TicketFields::default();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("##") {
            break;
        }
        let field = |prefix: &str| {
            line.get(..prefix.len())
                .filter(|head| head.eq_ignore_ascii_case(prefix))
                .map(|_| line[prefix.len()..].trim().to_owned())
        };
        if let Some(value) = field("Status:") {
            fields.status = Some(value);
        } else if let Some(value) = field("Type:") {
            fields.ty = Some(value);
        } else if let Some(value) = field("Blocked by:") {
            fields.blocked_by = value
                .split(',')
                .map(|item| item.trim().to_owned())
                .filter(|item| !item.is_empty())
                .collect();
        }
    }
    Ok(fields)
}

/// Split a ticket file into its `---`-delimited YAML frontmatter fields and
/// the Markdown body after them.
fn parse_older_dialect(content: &str) -> Result<(TicketFields, &str), String> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Err("no frontmatter".to_owned());
    };
    let Some((raw_fields, body)) = rest.split_once("\n---") else {
        return Err("unterminated frontmatter".to_owned());
    };
    let mut fields = TicketFields::default();
    for (key, value) in parse_frontmatter_fields(raw_fields)? {
        match (key.as_str(), value) {
            ("status", FieldValue::Scalar(value)) => fields.status = Some(value),
            ("type", FieldValue::Scalar(value)) => fields.ty = Some(value),
            ("assignee", FieldValue::Scalar(value)) => fields.assignee = Some(value),
            ("blocked-by", FieldValue::List(items)) => fields.blocked_by = items,
            ("external-blocked-by", FieldValue::List(items)) => {
                fields.external_blocked_by = items;
            }
            // An empty scalar where a list belongs is an empty list.
            ("blocked-by" | "external-blocked-by", FieldValue::Scalar(value))
                if value.is_empty() => {}
            (key @ ("status" | "type" | "assignee"), FieldValue::List(_)) => {
                return Err(format!(
                    "frontmatter key {key:?} holds a list, expected a value"
                ));
            }
            (key @ ("blocked-by" | "external-blocked-by"), FieldValue::Scalar(_)) => {
                return Err(format!(
                    "frontmatter key {key:?} holds a value, expected a list"
                ));
            }
            // Unknown keys pass through: tolerant parsing (spec §3).
            _ => {}
        }
    }
    Ok((fields, body))
}

/// A frontmatter value: a scalar (quotes stripped) or a list (inline
/// `[a, b]` flow style or indented `- item` block style — both observed).
enum FieldValue {
    Scalar(String),
    List(Vec<String>),
}

/// Hand-parsed `key: value` frontmatter — the corpus's narrow YAML subset
/// (scalars and string/int lists), deliberately not a YAML engine: every
/// observed file fits, and a line outside the subset degrades the Effort
/// rather than guessing.
fn parse_frontmatter_fields(raw: &str) -> Result<Vec<(String, FieldValue)>, String> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut fields = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            return Err(format!("malformed frontmatter line {line:?}"));
        };
        let key = raw_key.trim().to_owned();
        let raw_value = raw_value.trim();
        let value = if raw_value.is_empty() {
            // Either an empty value or the head of a block list.
            let mut items = Vec::new();
            while let Some(item) = lines
                .get(i)
                .map(|line| line.trim())
                .and_then(|line| line.strip_prefix('-'))
            {
                items.push(unquote(item.trim()).to_owned());
                i += 1;
            }
            if items.is_empty() {
                FieldValue::Scalar(String::new())
            } else {
                FieldValue::List(items)
            }
        } else if let Some(inner) = raw_value.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| format!("unterminated list in frontmatter line {line:?}"))?;
            FieldValue::List(
                inner
                    .split(',')
                    .map(|item| unquote(item.trim()).to_owned())
                    .filter(|item| !item.is_empty())
                    .collect(),
            )
        } else {
            FieldValue::Scalar(unquote(raw_value).to_owned())
        };
        fields.push((key, value));
    }
    Ok(fields)
}

/// Strip one matching pair of surrounding quotes, YAML-style.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests;
