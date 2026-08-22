# GitHub repository settings

Issue [#19](https://github.com/Lanque/MediaIndex/issues/19) tracks enforceable protection for `main`.

## Desired policy

When the repository plan supports branch protection for this private repository, configure a branch ruleset or branch protection rule targeting `main`:

- require a pull request before merging;
- require the `Repository checks` status check from `.github/workflows/ci.yml`;
- require the branch to be up to date before merging after CI is stable;
- require all conversations to be resolved;
- block force-pushes and branch deletion;
- keep `main` as the default branch;
- do not allow routine direct pushes;
- decide explicitly whether administrators may bypass the rule.

## GitHub UI path

Use either:

- **Settings → Branches → Add branch protection rule**, or
- **Settings → Rules → Rulesets → New branch ruleset**.

Select `main` as the target branch and save the policy only after confirming the CI check appears in GitHub’s required-status-check picker.

## Current limitation

GitHub currently returns HTTP 403 for the branch-protection and repository-rulesets APIs on this private repository with the message:

`Upgrade to GitHub Pro or make this repository public to enable this feature.`

No visibility change was made automatically. The policy is documented and tracked so it can be enabled without redesigning the workflow later.
