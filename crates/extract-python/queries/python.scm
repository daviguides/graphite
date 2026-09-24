; Graphite capture contract (shared across languages — downstream code keys on
; capture names only, never on grammar node types):
;
;   @definition.class / @definition.function   whole definition node
;   @name                                       its identifier
;   @definition.bases                           superclass list (optional)
;   @reference.call    + @reference.callee      call site + callee expression
;   @reference.decorator                        decorator expression
;   @import            + @import.module         `import a.b`
;                      + @import.alias          `... as c`
;                      + @import.from           `from x import ...`
;                      + @import.name           imported member
;                      + @import.wildcard       `from x import *`
;   @export.list       + @export.name           module-level `__all__ = [...]`

(class_definition
  name: (identifier) @name
  superclasses: (argument_list)? @definition.bases) @definition.class

(function_definition
  name: (identifier) @name) @definition.function

(call
  function: (_) @reference.callee) @reference.call

(decorator
  (_) @reference.decorator)

(import_statement
  name: (dotted_name) @import.module) @import

(import_statement
  name: (aliased_import
    name: (dotted_name) @import.module
    alias: (identifier) @import.alias)) @import

(import_from_statement
  module_name: (_) @import.from
  name: (dotted_name) @import.name) @import

(import_from_statement
  module_name: (_) @import.from
  name: (aliased_import
    name: (dotted_name) @import.name
    alias: (identifier) @import.alias)) @import

(import_from_statement
  module_name: (_) @import.from
  (wildcard_import) @import.wildcard) @import

(module
  (expression_statement
    (assignment
      left: (identifier) @export.name
      right: [(list) (tuple)] @export.list))
  (#eq? @export.name "__all__"))
