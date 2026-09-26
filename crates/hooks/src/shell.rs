//! Minimal shell splitting: top-level `;`/`&&`/`||`/newline and `|`, quote-aware. Anything the
//! splitter cannot reason about (substitution, subshells, heredocs, file redirects, background jobs)
//! makes the whole command unsupported, so it runs untouched.

/// How a segment is joined to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    First,
    Seq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub join: Join,
    /// Pipeline stages as raw text, stderr redirects (`2>/dev/null`, `2>&1`) stripped.
    pub stages: Vec<String>,
    /// The segment as written, for display and fallback execution.
    pub raw: String,
}

/// Split `cmd`; None when it uses constructs this module does not model.
pub fn split(cmd: &str) -> Option<Vec<Segment>> {
    let b = cmd.as_bytes();
    let mut segs = Vec::new();
    let mut stages: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut raw_start = 0usize;
    let mut join = Join::First;
    let (mut sq, mut dq) = (false, false);
    let mut i = 0;
    let finish_stage = |stages: &mut Vec<String>, cur: &mut String| -> bool {
        let t = cur.trim().to_string();
        cur.clear();
        if t.is_empty() {
            return false;
        }
        stages.push(t);
        true
    };
    while i < b.len() {
        let c = b[i] as char;
        if sq {
            cur.push(c);
            if c == '\'' {
                sq = false;
            }
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < b.len() {
            if dq && b[i + 1] == b'`' {
                return None;
            }
            cur.push(c);
            cur.push(b[i + 1] as char);
            i += 2;
            continue;
        }
        if dq {
            if c == '"' {
                dq = false;
            } else if c == '`' || (c == '$' && b.get(i + 1) == Some(&b'(')) {
                return None;
            }
            cur.push(c);
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                sq = true;
                cur.push(c);
            }
            '"' => {
                dq = true;
                cur.push(c);
            }
            '`' | '(' | ')' => return None,
            '$' if b.get(i + 1) == Some(&b'(') => return None,
            '<' => return None,
            '>' => {
                // Only stderr redirects are modeled: `2>/dev/null` and `2>&1`.
                if cur.ends_with('2') && (cur.len() == 1 || cur.as_bytes()[cur.len() - 2] == b' ') {
                    let rest = &cmd[i + 1..];
                    let len = if rest.starts_with("/dev/null") {
                        9
                    } else if rest.starts_with("&1") {
                        2
                    } else {
                        return None;
                    };
                    cur.pop();
                    i += 1 + len;
                    continue;
                }
                return None;
            }
            '&' if b.get(i + 1) == Some(&b'&') => {
                if !finish_stage(&mut stages, &mut cur) {
                    return None;
                }
                segs.push(Segment {
                    join,
                    stages: std::mem::take(&mut stages),
                    raw: cmd[raw_start..i].trim().to_string(),
                });
                join = Join::And;
                i += 2;
                raw_start = i;
                continue;
            }
            '&' => return None,
            '|' if b.get(i + 1) == Some(&b'|') => {
                if !finish_stage(&mut stages, &mut cur) {
                    return None;
                }
                segs.push(Segment {
                    join,
                    stages: std::mem::take(&mut stages),
                    raw: cmd[raw_start..i].trim().to_string(),
                });
                join = Join::Or;
                i += 2;
                raw_start = i;
                continue;
            }
            '|' => {
                if !finish_stage(&mut stages, &mut cur) {
                    return None;
                }
            }
            ';' | '\n' => {
                if finish_stage(&mut stages, &mut cur) || !stages.is_empty() {
                    segs.push(Segment {
                        join,
                        stages: std::mem::take(&mut stages),
                        raw: cmd[raw_start..i].trim().to_string(),
                    });
                    join = Join::Seq;
                }
                raw_start = i + 1;
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    if sq || dq {
        return None;
    }
    if finish_stage(&mut stages, &mut cur) {
        segs.push(Segment {
            join,
            stages,
            raw: cmd[raw_start..].trim().to_string(),
        });
    } else if !stages.is_empty() {
        return None;
    }
    Some(segs)
}

/// Words of one stage (quotes removed, no glob expansion).
pub fn words(stage: &str) -> Option<Vec<String>> {
    shlex::split(stage)
}

/// Quote one word for a POSIX shell.
pub fn quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(cmd: &str) -> Vec<Vec<String>> {
        split(cmd).unwrap().into_iter().map(|s| s.stages).collect()
    }

    #[test]
    fn splits_sequences_and_pipes() {
        assert_eq!(
            stages("grep -rn x . | grep -v tests | head -50"),
            vec![vec!["grep -rn x .", "grep -v tests", "head -50"]]
        );
        let s = split("a && b || c; d").unwrap();
        let joins: Vec<Join> = s.iter().map(|x| x.join).collect();
        assert_eq!(joins, vec![Join::First, Join::And, Join::Or, Join::Seq]);
        assert_eq!(stages("grep 'a|b' ."), vec![vec!["grep 'a|b' ."]]);
        assert_eq!(stages("grep \"a;b\" ."), vec![vec!["grep \"a;b\" ."]]);
    }

    #[test]
    fn strips_stderr_redirects_only() {
        assert_eq!(
            stages("grep -rn x . 2>/dev/null"),
            vec![vec!["grep -rn x ."]]
        );
        assert_eq!(stages("rg x 2>&1 | head"), vec![vec!["rg x", "head"]]);
        assert!(split("grep x . > out.txt").is_none());
        assert!(split("grep x < f").is_none());
    }

    #[test]
    fn refuses_unmodeled_constructs() {
        for c in [
            "echo $(ls)",
            "echo `ls`",
            "(cd a && ls)",
            "sleep 1 &",
            "cat <<EOF",
            "echo \"$(x)\"",
        ] {
            assert!(split(c).is_none(), "{c}");
        }
    }

    #[test]
    fn words_keep_globs_literal() {
        assert_eq!(
            words("grep -rn resolve_owner --include=*.py .").unwrap(),
            vec!["grep", "-rn", "resolve_owner", "--include=*.py", "."]
        );
    }
}
