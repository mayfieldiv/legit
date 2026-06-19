use std::io::{self, Write};

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let sequence = osc52_sequence(text);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(sequence.as_bytes())
        .context("failed to write OSC 52 clipboard sequence")?;
    stdout
        .flush()
        .context("failed to flush OSC 52 clipboard sequence")
}

fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

#[cfg(test)]
mod tests {
    use super::osc52_sequence;

    #[test]
    fn osc52_sequence_base64_encodes_clipboard_text() {
        assert_eq!(
            osc52_sequence("/tmp/legit"),
            "\x1b]52;c;L3RtcC9sZWdpdA==\x07"
        );
    }
}
