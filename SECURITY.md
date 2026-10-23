# Security Policy

## Supported versions

Ferropress has not made a release yet. Once releases exist, this section will list which versions receive
security fixes.

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report vulnerabilities privately through GitHub's **"Report a vulnerability"** button on the repository's
Security tab (private vulnerability reporting). Include:

- affected component and commit or version;
- steps to reproduce, or a proof of concept;
- impact as you understand it.

We aim to acknowledge reports within 3 business days and to agree on a disclosure timeline with the reporter.

## Scope notes

Ferropress executes third-party PHP plugin and theme code. A plugin that is malicious or vulnerable on a
stock WordPress installation is not automatically a Ferropress vulnerability; issues where Ferropress
behaves less securely than the reference WordPress behavior, or where isolation between sites or requests
fails, are in scope.
