# Personal data

No personal data stands in this repository: not a person's name, handle or account id, not a mail
address, host name, home path, domain, device or other network detail, and not a line of an actual
conversation. None of them stands in code, a comment, a document, a test fixture or a commit message.

## What it is

- A person's name, handle, account id or mail address.
- A host name, a domain, a home path, a device or a network address.
- A room member's display name and id.
- A company's name.
- A line of a real conversation.

## What to write instead

- A value that differs per installation, per person or per room is settings the consumer passes in; the
  code that reads it never carries its value.
- An example is neutral. A person is `tester`, `qa-admin` or `someone`; a mailbox stands at a reserved
  example domain; a home path names `user`; a token is written in lowercase, where a generated secret is
  mixed case.
- A test's fixture carries no real person, account or message. A conversation line a test reads is
  written for the test.

## The values the check admits

The reserved example domains (`example.com`, `example.net` and `example.org`, and the `.example`,
`.test`, `.invalid` and `.localhost` suffixes), the neutral home folders `user`, `tester`, `someone`,
`qa-admin`, `placeholder` and `example`, and a token written in lowercase as a placeholder.

## The check

The testbed's `personal-data` check, which the declaration enables, reads every file git tracks or would
track and fails where it finds a mail address, a home path that names an account, a telephone number, a
token or a private network address. A finding names the file, the line and the kind, never the value it found.

The check keeps the forms, not a list of names or values to look for: such a list would itself be the
personal data the rule keeps out.

`tests/repository.rs` holds the names half: it reads the names no repository keeps from the list the
`BMO_NAMES_LIST` variable names, and refuses each of them across the tracked files.
