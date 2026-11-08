# Nginx grammar: highlight classes it lacks

tree-sitter-nginx at 7cdf006ed1addef698fc680ae31721950f85e5e9 (v1.0.1) has no
function, type, escape or injection nodes in `node-types.json`, and upstream's
`queries/highlights.scm` captures none of them. The golden file therefore
colours comments, strings, numbers, keywords, constants, variables, operators
and punctuation only; no unrelated token is relabelled to fill a missing class.

Classes the grammar lacks: function, type, string escape, injected languages
(upstream's one injection targets a `comment` language this app doesn't have,
so `injections.scm` is comment-only).

Parse limits the sample works around: bare IPv4 values (`127.0.0.1`,
`listen 127.0.0.1:80`), `upstream` blocks and `""` map keys fail to parse
(`''` is fine).
