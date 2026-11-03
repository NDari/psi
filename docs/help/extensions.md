Extensions: Lua code that adds tools, commands, keys, UI and providers.

An extension is a .lua file or a directory with init.lua, returning
{ name, version, description, setup = function(psi) ... end }. They load from
~/.psi/extensions, the project's .psi/extensions (once trusted), --extension <path>,
and packages. A script tool is a directory with tool.json and an executable that reads
JSON arguments on stdin. /reload loads them again.

Batteries: psi.exec runs a command inside the sandbox, asking for always-ask commands
(io.popen and os.execute are not available); psi.http, psi.json, psi.re, psi.fs,
psi.path, psi.toml, psi.base64, psi.hash and psi.time cover the rest. An extension that
must run commands outside the sandbox needs "extensions": {"<name>": {"unsandboxed":
true}} in your settings.

psi ext check <path> lints one; psi ext test <path> runs its *_test.lua files;
psi ext example lists tested examples and psi ext example <name> prints one. The
author guide is docs/extensions.md and the API reference docs/api.md in the psi
repository.
