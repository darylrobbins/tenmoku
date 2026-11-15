# T639: XML doc comment semantic tokens

`highlight_name` maps Roslyn's `xmlDocComment*` token types, in both the LSP custom
spelling and the raw classification spelling (`xml doc comment - attribute name`), to
`tag`, `attribute`, `string`, `comment.documentation`, `comment`, `markup.raw`, `escape`
and `keyword` (all in fe-syntax's `NAMES`). Modifiers never change the answer.

## Verified (2026-11-15, dotnet/roslyn and razzmatazz/csharp-language-server, main)

- Roslyn `CustomLspSemanticTokenNames.cs` defines exactly the ten LSP custom names
  `xmlDocComment{Name,AttributeName,AttributeValue,AttributeQuotes,Delimiter,Text,Comment,
  CDataSection,EntityReference,ProcessingInstruction}`, and
  `ClassificationTypeNameToCustomTokenName` maps each `ClassificationTypeNames.XmlDocComment*`
  to them; `excludedCode` likewise. The raw spellings are the classification names
  (`xml doc comment - ...`).
- csharp-ls (`Handlers/SemanticTokens.fs`) does NOT send these types: its legend is only
  class, comment, constant, keyword, enum, enumMember, event, method, field, interface,
  variable, namespace, number, operator, parameter, property, struct, string,
  typeParameter, regexText, stringEscapeCharacter and json* types, and it has no mapping for
  XML doc comment or ExcludedCode classifications (spans without a type are dropped).
  So this mapping helps Roslyn's own server (a manual `[lsp.servers]` entry) only; with
  csharp-ls the grammar keeps colouring doc comments.
