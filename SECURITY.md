# Security policy

Kiln manages DNS entries, a local certificate authority and a privileged helper, so security
reports are taken seriously.

## Reporting a vulnerability

Please **do not** open a public issue. Report privately by email to
**sameed@tecclubx.com**, or via GitHub's "Report a vulnerability" (private security advisories)
on [sameedkun/kiln](https://github.com/sameedkun/kiln/security/advisories/new).

Include affected versions, OS, reproduction steps and impact. We aim to acknowledge within
3 working days.

## Scope highlights

- The privileged helper and its request validation
- The local CA (key protection, name constraints) and certificate issuance
- Any listener reachable from outside the machine
- Download and update verification
