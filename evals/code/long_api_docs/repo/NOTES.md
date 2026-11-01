# Team notes for docs/API.md

Rules for the API reference (agreed in the docs review):

- Document every public function of the modules in src/, one line each:
  `- module.function(args): one sentence saying what it does.`
- Group the lines under a `## module` heading per module, modules in
  alphabetical order, functions in the order they appear in the file.
- Public means: the name doesn't start with an underscore, and the line
  above the `def` is not `# internal`. Leave the others out.
- Skip everything under src/legacy/: that client is being deleted in the
  next release and must not appear in the reference.
- The file starts with the heading `# Shop backend API`.
- The last line of the file must be exactly:
  Reviewed-by: Ilse Marchetti (QX-4471), doc version 4.17.2
  (Ilse reviews the doc; QX-4471 is the docs ticket; 4.17.2 is the next
  release.)
