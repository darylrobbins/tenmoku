; csharp highlights, ours; goes after tree-sitter-c-sharp's HIGHLIGHTS_QUERY, which has no preprocessor captures.
["#if" "#elif" "#else" "#endif" "#region" "#endregion" "#define" "#undef" "#pragma"
 "#nullable" "#line" "#error" "#warning"] @keyword

; Members: a name that labels a value is a property. These come after the crate query's @variable.
(initializer_expression (assignment_expression left: (identifier) @property))
(with_initializer . (identifier) @property)
(argument name: (identifier) @property)
(attribute_argument name: (identifier) @property)
(member_access_expression name: (identifier) @property)
(invocation_expression function: (member_access_expression name: (identifier) @function))
(property_declaration name: (identifier) @property)
(field_declaration (variable_declaration (variable_declarator name: (identifier) @property)))
