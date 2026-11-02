/**
 * @file CSV and TSV, one grammar per separator, built so a query can count
 *   columns: every column is a named `field`, a direct child of its `row`.
 *
 * A `field` is its optional content followed by the separator; the last field
 * of a row has no separator, and an empty one has no node. So `a,,c` is three
 * fields, `,b` two and `a,` one.
 *
 * `csv/grammar.js` and `tsv/grammar.js` call this factory; see README.md.
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

/**
 * @param {string} name the language's name
 * @param {string} separator the one character between fields
 * @returns {ReturnType<typeof grammar>}
 */
module.exports = function columns(name, separator) {
  // The rest of a field: anything up to the separator or the end of the row.
  const rest = `[^${separator}\\r\\n]*`;

  return grammar({
    name,

    // Spaces and tabs belong to the fields; nothing is skipped.
    extras: _ => [],

    rules: {
      document: $ => seq(
        optional($.row),
        repeat(seq($._newline, optional($.row))),
      ),

      row: $ => choice(
        seq(repeat1($.field), optional(alias($._last_field, $.field))),
        alias($._last_field, $.field),
      ),

      field: $ => seq(optional($.content), separator),

      _last_field: $ => $.content,

      // A quoted field, where `""` reads as two strings in a row, or an unquoted
      // one. So nothing is a parse error, each also takes in what hand-written
      // files hold: `5" disk`, `"a"b`, and a quote never closed (the middle
      // line). An error would shift the columns after it, and an unclosed quote
      // would leave the rest of the file plain.
      content: _ => token(choice(
        new RegExp(`"[^"]*"("[^"]*")*${rest}`),
        new RegExp(`"${rest}`),
        new RegExp(`[^${separator}"\\r\\n]${rest}`),
      )),

      _newline: _ => /\r\n?|\n/,
    },
  });
};
