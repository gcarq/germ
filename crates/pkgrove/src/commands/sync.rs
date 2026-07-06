use log::error;
use pkgrove_core::repository::set::RepoSet;

/// Syncs all repositories.
pub fn sync(repo_set: &RepoSet) {
    for repo in repo_set.values() {
        match repo.sync() {
            Ok(()) => (),
            Err(e) => error!("failed to sync repository '{}'\n\t{e}", repo.name),
        }
    }
}
