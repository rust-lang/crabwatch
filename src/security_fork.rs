/// Returns whether a repository is a temporary private security-advisory fork.
pub fn is_security_advisory_fork(name: &str, is_private: bool) -> bool {
    // GitHub names these private forks `repo-ghsa-xxxx-xxxx-xxxx`.
    // https://docs.github.com/code-security/security-advisories/collaborating-in-a-temporary-private-fork-to-resolve-a-security-vulnerability
    is_private
        && name
            .rsplit_once("-ghsa-")
            .is_some_and(|(repository, advisory_id)| {
                !repository.is_empty() && is_ghsa_id_suffix(advisory_id)
            })
}

fn is_ghsa_id_suffix(value: &str) -> bool {
    const GHSA_ALPHABET: &[u8] = b"23456789cfghjmpqrvwx";

    let mut groups = value.split('-');
    let has_three_valid_groups = (0..3).all(|_| {
        groups.next().is_some_and(|group| {
            group.len() == 4
                && group
                    .bytes()
                    .all(|character| GHSA_ALPHABET.contains(&character))
        })
    });

    has_three_valid_groups && groups.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_security_advisory_fork() {
        assert!(is_security_advisory_fork(
            "example-ghsa-5wrv-w8mc-fq5j",
            true
        ));
    }

    #[test]
    fn retains_public_repository_with_advisory_name() {
        assert!(!is_security_advisory_fork(
            "example-ghsa-5wrv-w8mc-fq5j",
            false
        ));
    }

    #[test]
    fn rejects_invalid_security_advisory_names() {
        let names = [
            // Missing the `-ghsa-` separator and advisory ID.
            "private-repository",
            // The advisory ID contains characters outside the GHSA alphabet.
            "example-ghsa-abcd-1234-efgh",
            // The final advisory ID group has only three characters.
            "example-ghsa-5wrv-w8mc-fq5",
            // The `-ghsa-` separator is case-sensitive.
            "example-GHSA-5wrv-w8mc-fq5j",
            // Missing a repository name and its separating hyphen before `ghsa`.
            "ghsa-5wrv-w8mc-fq5j",
        ];

        for name in names {
            assert!(!is_security_advisory_fork(name, true), "accepted {name}");
        }
    }
}
