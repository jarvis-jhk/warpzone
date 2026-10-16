//! Tier-0 launch-file templates (`[tier0.files]` in a descriptor).
//!
//! `{{key}}` is replaced by a value; `{{#each others}} ... {{/each}}` repeats
//! its body once per other player, with the item's keys and `{{n}}` (1-based)
//! available inside. Unknown keys are an error, so a typo cannot silently
//! produce a broken launch file.
//!
//! Keys: `self.name`, `self.seat`, `self.host` (`true`/`false`),
//! `self.host_yesno` (`yes`/`no`), `game_port`, `seed`, `players`,
//! `setting.<key>`. Per other player: `name`, `seat`, `port` (the local
//! tunnel port that reaches them), `ip` (`127.0.0.1`), `n`.

use std::collections::BTreeMap;

#[derive(Default, Clone, Debug)]
pub struct Context {
    pub values: BTreeMap<String, String>,
    pub others: Vec<BTreeMap<String, String>>,
}

pub fn render(template: &str, ctx: &Context) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("unclosed {{")?;
        let tag = after[..end].trim();
        rest = &after[end + 2..];
        if let Some(list) = tag.strip_prefix("#each ") {
            if list.trim() != "others" {
                return Err(format!("unknown list {list}"));
            }
            let close = rest.find("{{/each}}").ok_or("missing {{/each}}")?;
            let body = &rest[..close];
            rest = &rest[close + "{{/each}}".len()..];
            for (i, item) in ctx.others.iter().enumerate() {
                let mut sub = Context { values: item.clone(), others: Vec::new() };
                sub.values.insert("n".into(), (i + 1).to_string());
                out.push_str(&render(body, &sub)?);
            }
        } else {
            let v = ctx.values.get(tag).ok_or_else(|| format!("unknown template key {tag}"))?;
            out.push_str(v);
        }
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_each() {
        let mut ctx = Context::default();
        ctx.values.insert("self.name".into(), "Alex".into());
        for (n, p) in [("Robin", "40001"), ("Bob", "40002")] {
            let mut m = BTreeMap::new();
            m.insert("name".into(), n.into());
            m.insert("port".into(), p.into());
            ctx.others.push(m);
        }
        let t = "[Settings]\nName={{self.name}}\n{{#each others}}[Other{{n}}]\nName={{name}}\nPort={{port}}\n{{/each}}";
        assert_eq!(render(t, &ctx).unwrap(), "[Settings]\nName=Alex\n[Other1]\nName=Robin\nPort=40001\n[Other2]\nName=Bob\nPort=40002\n");
        assert!(render("{{nope}}", &ctx).is_err());
    }
}
