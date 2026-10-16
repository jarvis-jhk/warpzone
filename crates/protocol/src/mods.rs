//! Mods: manifests, Factorio-style dependencies, the resolver, lockfiles and
//! offline validation.
//!
//! # Dependency syntax
//!
//! Each entry of a manifest's `dependencies` list is one string, exactly like
//! Factorio's `info.json`:
//!
//! | Entry | Meaning |
//! |---|---|
//! | `cave-tiles >= 1.2` | required, version constraint optional |
//! | `? music-pack` | optional: not installed automatically, but if present it must match the constraint and loads first |
//! | `(?) debug-overlay` | hidden optional: like `?`, not shown to players |
//! | `! old-tiles` | conflict: cannot be active at the same time (`! old-tiles < 2.0` conflicts only with older versions) |
//! | `~ shared-lib >= 1` | required, but does not affect load order |
//!
//! Operators: `<`, `<=`, `=`, `>=`, `>`. Versions are semantic versions;
//! `1.2` is read as `1.2.0`.
//!
//! # Where what happens
//!
//! Resolving (choosing versions) happens on the **server**, because it needs
//! the catalog and only happens while installing or updating, which needs the
//! network anyway. It returns a [`Lockfile`]: exact versions, hashes and a load
//! order. Clients only **validate** an enabled set offline
//! ([`validate_set`]), which needs no search.

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepKind {
    Required,
    Optional,
    HiddenOptional,
    Conflict,
    /// Required, without load-order effect (`~`).
    NoOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "=",
            Op::Ge => ">=",
            Op::Gt => ">",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub kind: DepKind,
    pub name: String,
    pub constraint: Option<(Op, Version)>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModError {
    #[error("invalid dependency `{0}`: {1}")]
    BadDependency(String, String),
    #[error("invalid version `{0}`")]
    BadVersion(String),
    #[error("invalid manifest: {0}")]
    BadManifest(String),
}

/// Parse `1`, `1.2` or `1.2.3` (and full semver) as a version.
pub fn parse_version(s: &str) -> Result<Version, ModError> {
    let s = s.trim();
    let dots = s.split(['-', '+']).next().unwrap_or("").matches('.').count();
    let padded = match dots {
        0 => format!("{s}.0.0"),
        1 => {
            let (core, rest) = split_core(s);
            format!("{core}.0{rest}")
        }
        _ => s.to_string(),
    };
    Version::parse(&padded).map_err(|_| ModError::BadVersion(s.into()))
}

fn split_core(s: &str) -> (&str, &str) {
    match s.find(['-', '+']) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

impl Dependency {
    pub fn parse(input: &str) -> Result<Dependency, ModError> {
        let bad = |why: &str| ModError::BadDependency(input.into(), why.into());
        let mut s = input.trim();
        let kind = if let Some(r) = s.strip_prefix("(?)") {
            s = r;
            DepKind::HiddenOptional
        } else if let Some(r) = s.strip_prefix('?') {
            s = r;
            DepKind::Optional
        } else if let Some(r) = s.strip_prefix('!') {
            s = r;
            DepKind::Conflict
        } else if let Some(r) = s.strip_prefix('~') {
            s = r;
            DepKind::NoOrder
        } else {
            DepKind::Required
        };
        s = s.trim_start();
        let name_end = s.find(|c: char| c.is_whitespace() || "<>=".contains(c)).unwrap_or(s.len());
        let name = &s[..name_end];
        if !crate::names::valid_mod_name(name) {
            return Err(bad("bad mod name"));
        }
        let rest = s[name_end..].trim();
        let constraint = if rest.is_empty() {
            None
        } else {
            let (op, v) = if let Some(v) = rest.strip_prefix("<=") {
                (Op::Le, v)
            } else if let Some(v) = rest.strip_prefix(">=") {
                (Op::Ge, v)
            } else if let Some(v) = rest.strip_prefix('<') {
                (Op::Lt, v)
            } else if let Some(v) = rest.strip_prefix('>') {
                (Op::Gt, v)
            } else if let Some(v) = rest.strip_prefix('=') {
                (Op::Eq, v)
            } else {
                return Err(bad("expected <, <=, =, >= or >"));
            };
            Some((op, parse_version(v).map_err(|_| bad("bad version"))?))
        };
        Ok(Dependency { kind, name: name.into(), constraint })
    }

    /// Whether `v` satisfies the version constraint (true without one).
    pub fn matches(&self, v: &Version) -> bool {
        match &self.constraint {
            None => true,
            Some((op, want)) => {
                let o = v.cmp_precedence(want);
                match op {
                    Op::Lt => o.is_lt(),
                    Op::Le => o.is_le(),
                    Op::Eq => o.is_eq(),
                    Op::Ge => o.is_ge(),
                    Op::Gt => o.is_gt(),
                }
            }
        }
    }

    pub fn is_required(&self) -> bool {
        matches!(self.kind, DepKind::Required | DepKind::NoOrder)
    }

    /// Whether this dependency puts its target before the dependent.
    pub fn orders(&self) -> bool {
        matches!(self.kind, DepKind::Required | DepKind::Optional | DepKind::HiddenOptional)
    }

    /// The constraint as text, without the prefix (`cave-tiles >= 2.0.0`).
    pub fn requirement(&self) -> String {
        match &self.constraint {
            None => self.name.clone(),
            Some((op, v)) => format!("{} {} {}", self.name, op.as_str(), v),
        }
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.kind {
            DepKind::Required => "",
            DepKind::Optional => "? ",
            DepKind::HiddenOptional => "(?) ",
            DepKind::Conflict => "! ",
            DepKind::NoOrder => "~ ",
        };
        write!(f, "{prefix}{}", self.requirement())
    }
}

/// The generic manifest a mod author submits next to the archive
/// (`warpzone-mod.json`). The server reads only this; game-specific data in
/// the archive is the game's business. Mirrors Factorio's `info.json`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub title: String,
    /// One of the game's categories (its descriptor's `mods.categories`).
    pub category: String,
    /// Compatible game versions, semver requirement syntax (`>=0.3, <0.5`).
    #[serde(default = "any")]
    pub game_version: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// SPDX identifier or a short licence name.
    pub license: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Game-specific fields; stored and passed on, never interpreted.
    #[serde(default)]
    pub extra: serde_json::Value,
}

fn any() -> String {
    "*".into()
}

impl Manifest {
    /// Validate and return the parsed version, game requirement and dependencies.
    pub fn check(&self) -> Result<(Version, VersionReq, Vec<Dependency>), ModError> {
        let bad = |m: &str| ModError::BadManifest(m.into());
        if !crate::names::valid_mod_name(&self.name) {
            return Err(bad("name must be 1-48 characters of a-z 0-9 _ -"));
        }
        if self.title.trim().is_empty() || self.title.len() > 80 {
            return Err(bad("title must be 1-80 characters"));
        }
        if self.description.len() > 4000 {
            return Err(bad("description is longer than 4000 characters"));
        }
        if self.license.trim().is_empty() || self.license.len() > 64 {
            return Err(bad("license is required"));
        }
        if self.tags.len() > 10 || self.tags.iter().any(|t| !crate::names::valid_slug(t)) {
            return Err(bad("up to 10 tags of a-z 0-9 _ - ."));
        }
        if self.dependencies.len() > 64 {
            return Err(bad("more than 64 dependencies"));
        }
        let version = Version::parse(&self.version).map_err(|_| bad("version must be semver (1.2.3)"))?;
        let game = VersionReq::parse(&self.game_version).map_err(|_| bad("game_version must be a semver requirement"))?;
        let deps = self.dependencies.iter().map(|d| Dependency::parse(d)).collect::<Result<Vec<_>, _>>()?;
        if deps.iter().any(|d| d.name == self.name) {
            return Err(bad("a mod cannot depend on itself"));
        }
        Ok((version, game, deps))
    }
}

/// One published version as the resolver sees it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub name: String,
    pub version: Version,
    pub deps: Vec<Dependency>,
    pub game_req: VersionReq,
    pub sha256: String,
    pub size: u64,
    pub category: String,
    pub yanked: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub size: u64,
    pub category: String,
}

/// An exact, verifiable mod set. Rooms carry its `hash`; everybody who joins
/// must run exactly this set.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Lockfile {
    pub format: u32,
    pub game: String,
    pub game_version: String,
    /// Sorted by name.
    pub mods: Vec<LockEntry>,
    /// Load order (dependencies first; ties alphabetical).
    pub order: Vec<String>,
    pub hash: String,
}

impl Lockfile {
    /// `sha256("warpzone-lock-1\n" game "\n" ("name@version=sha256\n")*)`, entries
    /// sorted by name. The game version is deliberately not part of it: rooms
    /// check the game version separately.
    pub fn compute_hash(game: &str, mods: &[LockEntry]) -> String {
        let mut sorted: Vec<&LockEntry> = mods.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let mut h = Sha256::new();
        h.update(b"warpzone-lock-1\n");
        h.update(game.as_bytes());
        h.update(b"\n");
        for m in sorted {
            h.update(format!("{}@{}={}\n", m.name, m.version, m.sha256).as_bytes());
        }
        hex::encode(h.finalize())
    }

    pub fn verify_hash(&self) -> bool {
        Self::compute_hash(&self.game, &self.mods) == self.hash
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolveError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Unsatisfiable(String),
    #[error("dependency cycle: {}", .0.join(" → "))]
    Cycle(Vec<String>),
    #[error("dependency graph too complex to resolve")]
    TooComplex,
}

const STEP_LIMIT: usize = 200_000;

#[derive(Clone)]
struct Pending {
    dep: Dependency,
    why: String,
}

struct Solver<'a> {
    catalog: &'a BTreeMap<String, Vec<Candidate>>,
    game_version: &'a Version,
    steps: usize,
    err: Option<(usize, String)>,
}

impl<'a> Solver<'a> {
    fn fail(&mut self, depth: usize, msg: String) {
        if self.err.as_ref().is_none_or(|(d, _)| depth >= *d) {
            self.err = Some((depth, msg));
        }
    }

    fn compatible(c: &Candidate, selected: &BTreeMap<String, &Candidate>) -> Result<(), String> {
        for d in &c.deps {
            if let Some(s) = selected.get(&d.name) {
                if d.kind == DepKind::Conflict {
                    if d.matches(&s.version) {
                        return Err(format!("{} {} conflicts with {} {}", c.name, c.version, s.name, s.version));
                    }
                } else if !d.matches(&s.version) {
                    return Err(format!("{} {} needs {} but {} {} is selected", c.name, c.version, d.requirement(), s.name, s.version));
                }
            }
        }
        for s in selected.values() {
            for d in s.deps.iter().filter(|d| d.name == c.name) {
                if d.kind == DepKind::Conflict {
                    if d.matches(&c.version) {
                        return Err(format!("{} {} conflicts with {} {}", s.name, s.version, c.name, c.version));
                    }
                } else if !d.matches(&c.version) {
                    return Err(format!("{} {} needs {}, not {}", s.name, s.version, d.requirement(), c.version));
                }
            }
        }
        Ok(())
    }

    fn solve(&mut self, selected: &mut BTreeMap<String, &'a Candidate>, pending: &[Pending], depth: usize) -> Result<bool, ResolveError> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            return Err(ResolveError::TooComplex);
        }
        let mut next = None;
        for p in pending {
            match selected.get(&p.dep.name) {
                Some(s) if p.dep.matches(&s.version) => continue,
                Some(s) => {
                    self.fail(depth, format!("{} needs {} but {} {} is selected", p.why, p.dep.requirement(), s.name, s.version));
                    return Ok(false);
                }
                None => {
                    next = Some(p.clone());
                    break;
                }
            }
        }
        let Some(p) = next else { return Ok(true) };
        let Some(versions) = self.catalog.get(&p.dep.name) else {
            self.fail(depth, format!("{} needs {}, which does not exist", p.why, p.dep.requirement()));
            return Ok(false);
        };
        let pinned = matches!(p.dep.constraint, Some((Op::Eq, _)));
        let mut cands: Vec<&'a Candidate> = versions
            .iter()
            .filter(|c| (!c.yanked || pinned) && p.dep.matches(&c.version))
            .collect();
        cands.sort_by(|a, b| b.version.cmp(&a.version));
        let game_ok: Vec<&'a Candidate> = cands.iter().copied().filter(|c| c.game_req.matches(self.game_version)).collect();
        if game_ok.is_empty() {
            let have: Vec<String> = versions.iter().filter(|c| !c.yanked).map(|c| c.version.to_string()).collect();
            let msg = if cands.is_empty() {
                format!("{} needs {} (available: {})", p.why, p.dep.requirement(), if have.is_empty() { "none".into() } else { have.join(", ") })
            } else {
                format!("{} needs {}, but no matching version supports game version {}", p.why, p.dep.requirement(), self.game_version)
            };
            self.fail(depth, msg);
            return Ok(false);
        }
        for c in game_ok {
            if let Err(msg) = Self::compatible(c, selected) {
                self.fail(depth + 1, msg);
                continue;
            }
            selected.insert(c.name.clone(), c);
            let mut more = pending.to_vec();
            let why = format!("{} → {} {}", p.why, c.name, c.version);
            more.extend(c.deps.iter().filter(|d| d.is_required()).map(|d| Pending { dep: d.clone(), why: why.clone() }));
            if self.solve(selected, &more, depth + 1)? {
                return Ok(true);
            }
            selected.remove(&c.name);
        }
        Ok(false)
    }
}

/// Choose versions for `wanted` (required dependencies, constraints optional)
/// from `catalog` (name → published, approved versions), newest first, with
/// backtracking. Returns a lockfile with a load order.
pub fn resolve(
    catalog: &BTreeMap<String, Vec<Candidate>>,
    wanted: &[Dependency],
    game: &str,
    game_version: &Version,
) -> Result<Lockfile, ResolveError> {
    if wanted.iter().any(|d| !d.is_required()) {
        return Err(ResolveError::Invalid("requested mods must be plain requirements".into()));
    }
    let mut solver = Solver { catalog, game_version, steps: 0, err: None };
    let pending: Vec<Pending> = wanted.iter().map(|d| Pending { dep: d.clone(), why: "requested".into() }).collect();
    let mut selected = BTreeMap::new();
    if !solver.solve(&mut selected, &pending, 0)? {
        let msg = solver.err.map(|(_, m)| m).unwrap_or_else(|| "no solution".into());
        return Err(ResolveError::Unsatisfiable(msg));
    }
    let chosen: Vec<&Candidate> = selected.values().copied().collect();
    let order = load_order(chosen.iter().map(|c| (c.name.as_str(), c.deps.as_slice())))?;
    let mods: Vec<LockEntry> = chosen
        .iter()
        .map(|c| LockEntry {
            name: c.name.clone(),
            version: c.version.to_string(),
            sha256: c.sha256.clone(),
            size: c.size,
            category: c.category.clone(),
        })
        .collect();
    let hash = Lockfile::compute_hash(game, &mods);
    Ok(Lockfile { format: 1, game: game.into(), game_version: game_version.to_string(), mods, order, hash })
}

/// Topological load order: a mod comes after everything it depends on with an
/// ordering dependency (required, optional, hidden optional) that is in the
/// set. Ties are broken alphabetically, so the order is deterministic.
pub fn load_order<'a>(mods: impl Iterator<Item = (&'a str, &'a [Dependency])>) -> Result<Vec<String>, ResolveError> {
    let mods: BTreeMap<&str, &[Dependency]> = mods.collect();
    let mut indegree: BTreeMap<&str, usize> = mods.keys().map(|k| (*k, 0)).collect();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, deps) in &mods {
        for d in deps.iter().filter(|d| d.orders() && mods.contains_key(d.name.as_str())) {
            *indegree.get_mut(name).unwrap() += 1;
            dependents.entry(d.name.as_str()).or_default().push(name);
        }
    }
    let mut ready: BTreeSet<&str> = indegree.iter().filter(|(_, n)| **n == 0).map(|(k, _)| *k).collect();
    let mut order = Vec::new();
    while let Some(n) = ready.pop_first() {
        order.push(n.to_string());
        for d in dependents.get(n).into_iter().flatten() {
            let e = indegree.get_mut(d).unwrap();
            *e -= 1;
            if *e == 0 {
                ready.insert(d);
            }
        }
    }
    if order.len() != mods.len() {
        let stuck: Vec<String> = indegree.iter().filter(|(_, n)| **n > 0).map(|(k, _)| k.to_string()).collect();
        return Err(ResolveError::Cycle(stuck));
    }
    Ok(order)
}

/// An installed mod as the client knows it (from its stored manifest).
#[derive(Debug, Clone)]
pub struct Installed {
    pub name: String,
    pub version: Version,
    pub deps: Vec<Dependency>,
    pub game_req: VersionReq,
}

/// Offline check of an enabled set: every requirement present and matching,
/// optional dependencies matching when present, no conflicts, game version
/// supported, no ordering cycle. Returns human-readable problems (empty = ok).
pub fn validate_set(enabled: &[Installed], game_version: &Version) -> Vec<String> {
    let by_name: BTreeMap<&str, &Installed> = enabled.iter().map(|m| (m.name.as_str(), m)).collect();
    let mut problems = Vec::new();
    if by_name.len() != enabled.len() {
        problems.push("the same mod is enabled twice".to_string());
    }
    for m in enabled {
        if !m.game_req.matches(game_version) {
            problems.push(format!("{} {} does not support game version {}", m.name, m.version, game_version));
        }
        for d in &m.deps {
            match (by_name.get(d.name.as_str()), d.kind) {
                (Some(o), DepKind::Conflict) if d.matches(&o.version) => {
                    problems.push(format!("{} {} conflicts with {} {}", m.name, m.version, o.name, o.version))
                }
                (Some(_), DepKind::Conflict) => {}
                (Some(o), _) if !d.matches(&o.version) => {
                    problems.push(format!("{} {} needs {}, but {} is enabled", m.name, m.version, d.requirement(), o.version))
                }
                (None, k) if matches!(k, DepKind::Required | DepKind::NoOrder) => {
                    problems.push(format!("{} {} needs {}, which is not enabled", m.name, m.version, d.requirement()))
                }
                _ => {}
            }
        }
    }
    if let Err(e) = load_order(enabled.iter().map(|m| (m.name.as_str(), m.deps.as_slice()))) {
        problems.push(e.to_string());
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(name: &str, v: &str, deps: &[&str]) -> Candidate {
        Candidate {
            name: name.into(),
            version: Version::parse(v).unwrap(),
            deps: deps.iter().map(|d| Dependency::parse(d).unwrap()).collect(),
            game_req: VersionReq::STAR,
            sha256: format!("{name}{v}"),
            size: 1,
            category: "x".into(),
            yanked: false,
        }
    }

    fn catalog(c: Vec<Candidate>) -> BTreeMap<String, Vec<Candidate>> {
        let mut m: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
        for x in c {
            m.entry(x.name.clone()).or_default().push(x);
        }
        m
    }

    fn want(s: &[&str]) -> Vec<Dependency> {
        s.iter().map(|d| Dependency::parse(d).unwrap()).collect()
    }

    fn gv() -> Version {
        Version::new(1, 0, 0)
    }

    #[test]
    fn parse_syntax() {
        let d = Dependency::parse("? music-pack >= 1.2").unwrap();
        assert_eq!(d.kind, DepKind::Optional);
        assert_eq!(d.constraint, Some((Op::Ge, Version::new(1, 2, 0))));
        assert_eq!(d.to_string(), "? music-pack >= 1.2.0");
        assert_eq!(Dependency::parse("(?) dbg").unwrap().kind, DepKind::HiddenOptional);
        assert_eq!(Dependency::parse("!old").unwrap().kind, DepKind::Conflict);
        assert_eq!(Dependency::parse("~ lib=2").unwrap().constraint, Some((Op::Eq, Version::new(2, 0, 0))));
        assert!(Dependency::parse("bad name").is_err());
        assert!(Dependency::parse("x ~> 1").is_err());
    }

    #[test]
    fn transitive_chain_newest_first() {
        let c = catalog(vec![
            cand("campaign", "1.0.0", &["world-one >= 1"]),
            cand("world-one", "1.0.0", &["level-one >= 2", "bat"]),
            cand("level-one", "2.0.0", &["cave-tiles"]),
            cand("level-one", "2.1.0", &["cave-tiles >= 2"]),
            cand("cave-tiles", "1.0.0", &[]),
            cand("cave-tiles", "2.0.0", &[]),
            cand("bat", "3.0.0", &[]),
        ]);
        let l = resolve(&c, &want(&["campaign"]), "g", &gv()).unwrap();
        let v: BTreeMap<_, _> = l.mods.iter().map(|m| (m.name.as_str(), m.version.as_str())).collect();
        assert_eq!(v["level-one"], "2.1.0");
        assert_eq!(v["cave-tiles"], "2.0.0");
        assert_eq!(l.order, vec!["bat", "cave-tiles", "level-one", "world-one", "campaign"]);
        assert!(l.verify_hash());
    }

    #[test]
    fn backtracks_when_newest_conflicts() {
        let c = catalog(vec![
            cand("a", "1.0.0", &["b", "c"]),
            cand("b", "2.0.0", &["! c"]),
            cand("b", "1.0.0", &[]),
            cand("c", "1.0.0", &[]),
        ]);
        let l = resolve(&c, &want(&["a"]), "g", &gv()).unwrap();
        assert!(l.mods.iter().any(|m| m.name == "b" && m.version == "1.0.0"));
    }

    #[test]
    fn explains_failure_with_chain() {
        let c = catalog(vec![
            cand("campaign", "1.0.0", &["world-one"]),
            cand("world-one", "1.0.0", &["cave-tiles >= 2"]),
            cand("cave-tiles", "1.0.0", &[]),
        ]);
        let e = resolve(&c, &want(&["campaign"]), "g", &gv()).unwrap_err().to_string();
        assert!(e.contains("requested → campaign 1.0.0 → world-one 1.0.0 needs cave-tiles >= 2.0.0"), "{e}");
        assert!(e.contains("available: 1.0.0"), "{e}");
    }

    #[test]
    fn optional_constrains_when_present() {
        let c = catalog(vec![
            cand("a", "1.0.0", &["? b >= 2"]),
            cand("b", "1.0.0", &[]),
            cand("b", "2.0.0", &[]),
        ]);
        let l = resolve(&c, &want(&["a"]), "g", &gv()).unwrap();
        assert_eq!(l.mods.len(), 1, "optional is not installed automatically");
        let l = resolve(&c, &want(&["a", "b"]), "g", &gv()).unwrap();
        assert!(l.mods.iter().any(|m| m.name == "b" && m.version == "2.0.0"));
        assert_eq!(l.order, vec!["b", "a"]);
        assert!(resolve(&c, &want(&["a", "b = 1.0"]), "g", &gv()).is_err());
    }

    #[test]
    fn game_version_and_yanked() {
        let mut new = cand("a", "2.0.0", &[]);
        new.game_req = VersionReq::parse(">=2").unwrap();
        let mut y = cand("a", "1.5.0", &[]);
        y.yanked = true;
        let c = catalog(vec![cand("a", "1.0.0", &[]), y, new]);
        let l = resolve(&c, &want(&["a"]), "g", &gv()).unwrap();
        assert_eq!(l.mods[0].version, "1.0.0");
        let l = resolve(&c, &want(&["a = 1.5"]), "g", &gv()).unwrap();
        assert_eq!(l.mods[0].version, "1.5.0", "an exact pin may select a yanked version");
    }

    #[test]
    fn cycles() {
        let c = catalog(vec![cand("a", "1.0.0", &["b"]), cand("b", "1.0.0", &["a"])]);
        assert!(matches!(resolve(&c, &want(&["a"]), "g", &gv()), Err(ResolveError::Cycle(_))));
        let c = catalog(vec![cand("a", "1.0.0", &["~ b"]), cand("b", "1.0.0", &["a"])]);
        assert_eq!(resolve(&c, &want(&["a"]), "g", &gv()).unwrap().order, vec!["a", "b"]);
    }

    #[test]
    fn offline_validation() {
        let i = |n: &str, v: &str, d: &[&str]| Installed {
            name: n.into(),
            version: Version::parse(v).unwrap(),
            deps: d.iter().map(|x| Dependency::parse(x).unwrap()).collect(),
            game_req: VersionReq::STAR,
        };
        assert!(validate_set(&[i("a", "1.0.0", &["b"]), i("b", "1.0.0", &[])], &gv()).is_empty());
        let p = validate_set(&[i("a", "1.0.0", &["b >= 2", "! c"]), i("c", "1.0.0", &[])], &gv());
        assert_eq!(p.len(), 2, "{p:?}");
    }

    #[test]
    fn manifest_check() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"cave-tiles","version":"1.0.0","title":"Cave tiles","category":"tileset",
                "game_version":">=0.3","dependencies":["bat >= 3","? music"],"license":"CC-BY-4.0"}"#,
        )
        .unwrap();
        let (v, g, d) = m.check().unwrap();
        assert_eq!(v, Version::new(1, 0, 0));
        assert!(g.matches(&Version::new(0, 4, 0)));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn versions_pad() {
        assert_eq!(parse_version("1").unwrap(), Version::new(1, 0, 0));
        assert_eq!(parse_version("1.2").unwrap(), Version::new(1, 2, 0));
        assert_eq!(parse_version("1.2-beta").unwrap().to_string(), "1.2.0-beta");
    }
}
