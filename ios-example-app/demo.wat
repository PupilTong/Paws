;; Minimal WASM demo for the Paws iOS renderer.
;;
;; Creates a root div with a CSS stylesheet that produces a scrollable
;; list of colored rows. Demonstrates the full pipeline:
;; WASM → DOM → Style → Layout → Renderer → Swift.

(module
  (import "env" "__CreateElement" (func $create (param i32) (result i32)))
  (import "env" "__SetInlineStyle" (func $set_style (param i32 i32 i32) (result i32)))
  (import "env" "__AppendElement" (func $append (param i32 i32) (result i32)))
  (import "env" "__AddStylesheet" (func $add_css (param i32) (result i32)))
  (memory (export "memory") 1)

  ;; String data in linear memory.
  ;; Tag names
  (data (i32.const 0)   "div\00")

  ;; CSS stylesheet — defines the layout for root, container, and rows.
  (data (i32.const 16)  ".root { display: block; width: 390px; height: 844px; } .container { display: block; width: 390px; height: 1600px; overflow-y: scroll; } .row { display: block; width: 390px; height: 80px; }\00")

  ;; Style property names and values
  (data (i32.const 512) "class\00")
  (data (i32.const 528) "root\00")
  (data (i32.const 544) "container\00")
  (data (i32.const 560) "row\00")

  ;; Inline style names/values for colored rows
  (data (i32.const 576) "background-color\00")
  (data (i32.const 600) "rgb(255,100,100)\00")
  (data (i32.const 624) "rgb(100,255,100)\00")
  (data (i32.const 648) "rgb(100,100,255)\00")
  (data (i32.const 672) "rgb(255,255,100)\00")

  (func (export "run") (result i32)
    (local $root i32)
    (local $container i32)
    (local $row1 i32)
    (local $row2 i32)
    (local $row3 i32)
    (local $row4 i32)

    ;; Add the stylesheet.
    (call $add_css (i32.const 16))
    (drop)

    ;; Create root div.
    (local.set $root (call $create (i32.const 0)))
    (call $append (i32.const 0) (local.get $root))
    (drop)

    ;; Create scroll container.
    (local.set $container (call $create (i32.const 0)))
    (call $append (local.get $root) (local.get $container))
    (drop)

    ;; Create 4 colored rows.
    (local.set $row1 (call $create (i32.const 0)))
    (call $set_style (local.get $row1) (i32.const 576) (i32.const 600))
    (drop)
    (call $append (local.get $container) (local.get $row1))
    (drop)

    (local.set $row2 (call $create (i32.const 0)))
    (call $set_style (local.get $row2) (i32.const 576) (i32.const 624))
    (drop)
    (call $append (local.get $container) (local.get $row2))
    (drop)

    (local.set $row3 (call $create (i32.const 0)))
    (call $set_style (local.get $row3) (i32.const 576) (i32.const 648))
    (drop)
    (call $append (local.get $container) (local.get $row3))
    (drop)

    (local.set $row4 (call $create (i32.const 0)))
    (call $set_style (local.get $row4) (i32.const 576) (i32.const 672))
    (drop)
    (call $append (local.get $container) (local.get $row4))
    (drop)

    ;; Return success.
    (i32.const 0)
  )
)
