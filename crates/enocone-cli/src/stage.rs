//! `enocone approve` and `enocone stage`: change a flow's stage in its frontmatter, nothing else.

use enocone_spec::Stage;

/// The flow's text with `stage:` set, and `approved_by:` set (`Some`), removed
/// (before approved), or kept. Every other line stays exactly as it was.
pub fn set_stage(text: &str, stage: Stage, approved_by: Option<&str>) -> String {
    let keep_approval = matches!(stage, Stage::Approved | Stage::Implemented);
    let (nl, body_start, fm) = match frontmatter(text) {
        Some((start, end)) => ("\n", end, Some(&text[start..end])),
        None => ("\n", 0, None),
    };
    let mut lines: Vec<String> = fm.map(|f| f.lines().map(str::to_string).collect()).unwrap_or_default();
    let key = |l: &str, k: &str| l.split_once(':').is_some_and(|(a, _)| a.trim() == k) && !l.starts_with([' ', '\t']);
    match lines.iter().position(|l| key(l, "stage")) {
        Some(i) => lines[i] = format!("stage: {}", stage.as_str()),
        None => lines.push(format!("stage: {}", stage.as_str())),
    }
    let at = lines.iter().position(|l| key(l, "approved_by"));
    match (approved_by, at) {
        (Some(by), Some(i)) => lines[i] = format!("approved_by: {by}"),
        (Some(by), None) => {
            let s = lines.iter().position(|l| key(l, "stage")).unwrap();
            lines.insert(s + 1, format!("approved_by: {by}"));
        }
        (None, Some(i)) if !keep_approval => {
            lines.remove(i);
        }
        _ => {}
    }
    let fm = lines.join(nl);
    if body_start == 0 { format!("---\n{fm}\n---\n\n{text}") } else { format!("---\n{fm}\n{}", &text[body_start..]) }
}

/// The frontmatter's inner text as (start, end), where `end` is where the closing `---` line begins.
fn frontmatter(text: &str) -> Option<(usize, usize)> {
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let start = text.len() - rest.len();
    let mut pos = start;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((start, pos));
        }
        pos += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_changes_only_frontmatter() {
        let src = "---\narea: Quotes\nstage: draft\ntags: [a]\n---\n\n# Title\n\nstage: draft in the body\n";
        let out = set_stage(src, Stage::Approved, Some("Priya Shah (2026-11-08)"));
        assert_eq!(out, "---\narea: Quotes\nstage: approved\napproved_by: Priya Shah (2026-11-08)\ntags: [a]\n---\n\n# Title\n\nstage: draft in the body\n");
        let back = set_stage(&out, Stage::Review, None);
        assert_eq!(back, src.replace("stage: draft\n", "stage: review\n"));
        let done = set_stage(&out, Stage::Implemented, None);
        assert!(done.contains("stage: implemented\napproved_by: Priya Shah"));
    }

    #[test]
    fn adds_frontmatter_when_missing() {
        let out = set_stage("# Title\n", Stage::Approved, Some("Ana (2026-11-08)"));
        assert_eq!(out, "---\nstage: approved\napproved_by: Ana (2026-11-08)\n---\n\n# Title\n");
    }
}
