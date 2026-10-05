//! Minimal shell splitting: top-level `;`/`&&`/`||`/newline and `|`, quote-aware. Anything the
//! splitter cannot reason about (subshells, heredocs, file redirects, background jobs, substitution
//! outside a plain `NAME=$(…)` assignment) makes the whole command unsupported, so it runs
//! untouched.

use std::collections::BTreeMap;

/// How a segment is joined to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    First,
    Seq,
    And,
    Or,
}

/// `NAME=$(inner)` — a whole segment that stores a command's output in a shell variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub name: String,
    /// The substituted command, as written.
    pub inner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub join: Join,
    /// Pipeline stages as raw text, stderr and `/dev/null` redirects stripped.
    pub stages: Vec<String>,
    /// The segment as written, for display and fallback execution.
    pub raw: String,
    /// Set when the segment is a `NAME=$(…)` assignment.
    pub assign: Option<Assign>,
    /// stdout goes to `/dev/null`: nothing of it reaches the agent.
    pub discard_stdout: bool,
}

/// Shell variables assigned earlier in the same command.
pub type Env = BTreeMap<String, String>;

fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && c.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Parse `NAME=$(inner)` / `NAME="$(inner)"` whose `$` is at `dollar`. `prefix` is the segment text
/// before it. Returns the assignment and the index just past it; None for any other shape (nested
/// substitution, subshell parens, text glued after the closing paren).
fn assignment(cmd: &str, dollar: usize, prefix: &str, quoted: bool) -> Option<(Assign, usize)> {
    let name = prefix
        .trim_start()
        .strip_suffix(if quoted { "=\"" } else { "=" })?;
    if !is_name(name) {
        return None;
    }
    let b = cmd.as_bytes();
    let (mut sq, mut dq) = (false, false);
    let mut i = dollar + 2;
    let close = loop {
        let c = *b.get(i)? as char;
        match c {
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            '\\' if !sq => i += 1,
            '`' if !sq => return None,
            '$' if !sq && b.get(i + 1) == Some(&b'(') => return None,
            '(' if !sq && !dq => return None,
            ')' if !sq && !dq => break i,
            _ => {}
        }
        i += 1;
    };
    let mut end = close + 1;
    if quoted {
        if b.get(end) != Some(&b'"') {
            return None;
        }
        end += 1;
    }
    let rest = cmd[end..].trim_start_matches([' ', '\t']);
    let terminated = rest.is_empty()
        || rest.starts_with([';', '\n'])
        || rest.starts_with("&&")
        || rest.starts_with("||");
    if !terminated {
        return None;
    }
    let inner = cmd[dollar + 2..close].trim().to_string();
    (!inner.is_empty()).then(|| {
        (
            Assign {
                name: name.to_string(),
                inner,
            },
            end,
        )
    })
}

/// Length of a `/dev/null` target at the start of `rest` (spaces allowed before it), if any.
fn dev_null(rest: &str) -> Option<usize> {
    let skipped = rest.len() - rest.trim_start_matches(' ').len();
    let tail = &rest[skipped..];
    let after = tail.strip_prefix("/dev/null")?;
    after
        .chars()
        .next()
        .is_none_or(|c| c.is_whitespace() || ";|&".contains(c))
        .then_some(skipped + "/dev/null".len())
}

/// Builds segments as the splitter walks the command.
#[derive(Default)]
struct Builder {
    segs: Vec<Segment>,
    stages: Vec<String>,
    cur: String,
    raw_start: usize,
    join: Option<Join>,
    assign: Option<Assign>,
    discard: bool,
}

impl Builder {
    fn finish_stage(&mut self) -> bool {
        let t = self.cur.trim().to_string();
        self.cur.clear();
        if t.is_empty() {
            return false;
        }
        self.stages.push(t);
        true
    }

    fn push_segment(&mut self, cmd: &str, end: usize) {
        self.segs.push(Segment {
            join: self.join.unwrap_or(Join::First),
            stages: std::mem::take(&mut self.stages),
            raw: cmd[self.raw_start..end].trim().to_string(),
            assign: self.assign.take(),
            discard_stdout: std::mem::take(&mut self.discard),
        });
    }

    /// `&&` / `||`: the current segment must be non-empty.
    fn logical(&mut self, cmd: &str, i: usize, join: Join) -> Option<()> {
        let finished = self.finish_stage();
        if (!finished && self.assign.is_none()) || self.stages.is_empty() {
            return None;
        }
        self.push_segment(cmd, i);
        self.join = Some(join);
        self.raw_start = i + 2;
        Some(())
    }
}

/// Split `cmd`; None when it uses constructs this module does not model.
pub fn split(cmd: &str) -> Option<Vec<Segment>> {
    let b = cmd.as_bytes();
    let mut s = Builder::default();
    let (mut sq, mut dq) = (false, false);
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if sq {
            s.cur.push(c);
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
            s.cur.push(c);
            s.cur.push(b[i + 1] as char);
            i += 2;
            continue;
        }
        if dq {
            if c == '"' {
                dq = false;
            } else if c == '$' && b.get(i + 1) == Some(&b'(') {
                if !s.stages.is_empty() {
                    return None;
                }
                let (a, end) = assignment(cmd, i, &s.cur, true)?;
                s.stages.push(cmd[s.raw_start..end].trim().to_string());
                s.cur.clear();
                s.assign = Some(a);
                dq = false;
                i = end;
                continue;
            } else if c == '`' {
                return None;
            }
            s.cur.push(c);
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                sq = true;
                s.cur.push(c);
            }
            '"' => {
                dq = true;
                s.cur.push(c);
            }
            '`' | '(' | ')' => return None,
            '$' if b.get(i + 1) == Some(&b'(') => {
                if !s.stages.is_empty() {
                    return None;
                }
                let (a, end) = assignment(cmd, i, &s.cur, false)?;
                s.stages.push(cmd[s.raw_start..end].trim().to_string());
                s.cur.clear();
                s.assign = Some(a);
                i = end;
                continue;
            }
            '<' => return None,
            '>' => {
                let digit_before = |d: u8| {
                    s.cur.as_bytes().last() == Some(&d)
                        && (s.cur.len() == 1 || s.cur.as_bytes()[s.cur.len() - 2] == b' ')
                };
                let rest = &cmd[i + 1..];
                if digit_before(b'2') {
                    // stderr: `2>/dev/null` and `2>&1` only.
                    let len = if let Some(n) = dev_null(rest) {
                        n
                    } else if rest.starts_with("&1") {
                        2
                    } else {
                        return None;
                    };
                    s.cur.pop();
                    i += 1 + len;
                    continue;
                }
                // stdout: only to /dev/null (`>/dev/null`, `1>/dev/null`, `>>/dev/null`).
                let (skip, rest) = match rest.strip_prefix('>') {
                    Some(r) => (1, r),
                    None => (0, rest),
                };
                let n = dev_null(rest)?;
                if digit_before(b'1') {
                    s.cur.pop();
                }
                s.discard = true;
                i += 1 + skip + n;
                continue;
            }
            '&' if b.get(i + 1) == Some(&b'>') => {
                let n = dev_null(&cmd[i + 2..])?;
                s.discard = true;
                i += 2 + n;
                continue;
            }
            '&' if b.get(i + 1) == Some(&b'&') => {
                s.logical(cmd, i, Join::And)?;
                i += 2;
                continue;
            }
            '&' => return None,
            '|' if b.get(i + 1) == Some(&b'|') => {
                s.logical(cmd, i, Join::Or)?;
                i += 2;
                continue;
            }
            '|' => {
                if !s.finish_stage() {
                    return None;
                }
            }
            ';' | '\n' => {
                if s.finish_stage() || !s.stages.is_empty() {
                    s.push_segment(cmd, i);
                    s.join = Some(Join::Seq);
                }
                s.raw_start = i + 1;
            }
            _ => s.cur.push(c),
        }
        i += 1;
    }
    if sq || dq {
        return None;
    }
    if s.finish_stage() || s.assign.is_some() {
        s.push_segment(cmd, cmd.len());
    } else if !s.stages.is_empty() {
        return None;
    }
    Some(s.segs)
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

/// Names of `env` variables `text` expands (`$NAME`, `${NAME}`, outside single quotes).
pub fn references(text: &str, env: &Env) -> Vec<String> {
    let mut out = Vec::new();
    scan_vars(text, |name, _| {
        if env.contains_key(name) && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    });
    out
}

/// Calls `f(name, in_double_quotes)` for every `$NAME` / `${NAME}` outside single quotes and
/// returns the byte spans of each reference.
fn scan_vars(text: &str, mut f: impl FnMut(&str, bool)) -> Vec<(usize, usize, String, bool)> {
    let b = text.as_bytes();
    let (mut sq, mut dq) = (false, false);
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' if !dq => sq = !sq,
            b'"' if !sq => dq = !dq,
            b'\\' if !sq => i += 1,
            b'$' if !sq => {
                let (start, braced) = if b.get(i + 1) == Some(&b'{') {
                    (i + 2, true)
                } else {
                    (i + 1, false)
                };
                let len = text[start..]
                    .bytes()
                    .take_while(|c| *c == b'_' || c.is_ascii_alphanumeric())
                    .count();
                let name = &text[start..start + len];
                let closed = !braced || b.get(start + len) == Some(&b'}');
                if is_name(name) && closed {
                    let end = start + len + usize::from(braced);
                    f(name, dq);
                    spans.push((i, end, name.to_string(), dq));
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    spans
}

/// `text` with every `env` variable replaced by its value, quoted the way the shell would see it:
/// unquoted values split into words, each quoted; values inside double quotes stay one word.
/// Variables not in `env` are left as written.
pub fn expand(text: &str, env: &Env) -> String {
    let spans = scan_vars(text, |_, _| {});
    let mut out = String::new();
    let mut last = 0;
    for (start, end, name, in_dq) in spans {
        let Some(value) = env.get(&name) else {
            continue;
        };
        out.push_str(&text[last..start]);
        if in_dq {
            for ch in value.chars() {
                if matches!(ch, '"' | '\\' | '$' | '`') {
                    out.push('\\');
                }
                out.push(ch);
            }
        } else {
            let words: Vec<String> = value.split_whitespace().map(quote).collect();
            out.push_str(&words.join(" "));
        }
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// `NAME='value'; ` prefixes that recreate `env` for a segment run by `/bin/sh`.
pub fn env_prefix(env: &Env) -> String {
    env.iter()
        .map(|(k, v)| format!("{k}={}; ", quote(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(cmd: &str) -> Vec<Vec<String>> {
        split(cmd).unwrap().into_iter().map(|s| s.stages).collect()
    }

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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

    // kinhin: decision(ref="docs/foundation/interception.md#1-steering-by-transparent-interception")
    #[test]
    fn stdout_to_dev_null_is_a_discarded_segment_not_a_file_write() {
        for cmd in [
            "git stash list >/dev/null",
            "git stash list > /dev/null",
            "git stash list 1>/dev/null",
            "git stash list &>/dev/null",
            "git stash list >/dev/null 2>&1",
            "git stash list >>/dev/null",
        ] {
            let s = split(cmd).unwrap_or_else(|| panic!("{cmd}"));
            assert_eq!(s[0].stages, vec!["git stash list"], "{cmd}");
            assert!(s[0].discard_stdout, "{cmd}");
        }
        assert!(!split("git stash list 2>/dev/null").unwrap()[0].discard_stdout);
        for cmd in ["ls > /dev/nullx", "ls &> out", "ls >> log.txt"] {
            assert!(split(cmd).is_none(), "{cmd}");
        }
    }

    // kinhin: decision(ref="docs/foundation/interception.md#1-steering-by-transparent-interception")
    #[test]
    fn assignment_from_substitution_is_its_own_segment() {
        let s = split("f=$(find . -name x.py | head -1); echo $f; cat -n \"$f\"").unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(
            s[0].assign,
            Some(Assign {
                name: "f".into(),
                inner: "find . -name x.py | head -1".into()
            })
        );
        assert_eq!(s[1].stages, vec!["echo $f"]);
        assert_eq!(s[2].stages, vec!["cat -n \"$f\""]);
        let q = split("f=\"$(rg -l x)\" && cat $f").unwrap();
        assert_eq!(q[0].assign.as_ref().unwrap().inner, "rg -l x");
        assert_eq!(q[1].join, Join::And);
        for cmd in [
            "echo $(ls)",
            "f=$(ls) cat $f",
            "f=$(ls)x; cat $f",
            "f=$(echo $(ls))",
            "f=$( (ls) )",
            "1f=$(ls)",
            "f=$(ls) | cat",
            "echo `ls`",
            "(cd a && ls)",
            "sleep 1 &",
            "cat <<EOF",
            "echo \"$(x)\"",
        ] {
            assert!(split(cmd).is_none(), "{cmd}");
        }
    }

    // kinhin: decision(ref="docs/foundation/interception.md#1-steering-by-transparent-interception")
    #[test]
    fn expansion_quotes_values_the_way_the_shell_splits_them() {
        let e = env(&[("f", "./a b.py"), ("g", "x'y")]);
        assert_eq!(expand("cat $f", &e), "cat ./a b.py");
        assert_eq!(expand("cat \"$f\"", &e), "cat \"./a b.py\"");
        assert_eq!(expand("cat ${f}", &e), "cat ./a b.py");
        assert_eq!(expand("echo '$f'", &e), "echo '$f'");
        assert_eq!(expand("cat $g", &e), "cat 'x'\\''y'");
        assert_eq!(expand("echo $HOME $f", &e), "echo $HOME ./a b.py");
        assert_eq!(references("wc -l $f; echo $HOME '$g'", &e), vec!["f"]);
        assert_eq!(env_prefix(&e), "f='./a b.py'; g='x'\\''y'; ");
    }

    #[test]
    fn words_keep_globs_literal() {
        assert_eq!(
            words("grep -rn resolve_owner --include=*.py .").unwrap(),
            vec!["grep", "-rn", "resolve_owner", "--include=*.py", "."]
        );
    }
}
