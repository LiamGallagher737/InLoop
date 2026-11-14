//! Installing hooks from `inloop init`.

use crate::init::InitReport;

pub fn install(repo: &inloop_git::Repo, r: &mut InitReport) -> anyhow::Result<()> {
    let mut h = inloop_app::hooks::HookReport::default();
    inloop_app::hooks::install_claude_hooks(&repo.root, &mut h)?;
    inloop_app::hooks::install_git_hook(repo, &mut h)?;
    r.created.extend(h.created);
    r.updated.extend(h.updated);
    r.unchanged.extend(h.unchanged);
    r.warnings.extend(h.warnings);
    Ok(())
}
