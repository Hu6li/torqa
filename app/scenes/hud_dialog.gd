class_name HudDialog
extends ConfirmationDialog
## The HUD editor (R51) as a dialog during the ride; applies to the active rider's layout.

## The chosen metric ids, the large one first.
signal layout_confirmed(layout: PackedStringArray)

var _editor: HudEditor = HudEditor.new()


func _ready() -> void:
	theme = UiTheme.build()
	title = "Customize HUD"
	ok_button_text = "Apply"
	min_size = Vector2i(860, 480)
	add_child(_editor)
	confirmed.connect(func() -> void: layout_confirmed.emit(_editor.layout()))


## Opens the editor on `layout`, in the rider's units.
func edit(layout: PackedStringArray, imperial: bool) -> void:
	_editor.edit(layout, imperial)
	popup_centered(Vector2i(960, 560))
