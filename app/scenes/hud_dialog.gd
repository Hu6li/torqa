class_name HudDialog
extends ConfirmationDialog
## Chooses the ride HUD's metrics: one shown large, any others in the grid below it (R23).

## The chosen metric ids, the large one first.
signal layout_confirmed(layout: PackedStringArray)

## Most metrics in the grid; the core caps the whole layout as well.
const MAX_SMALL: int = 12

var _main: OptionButton = OptionButton.new()
var _checks: Dictionary[String, CheckBox] = {}
var _order: PackedStringArray = PackedStringArray()
var _grid: GridContainer = GridContainer.new()
var _hint: Label = Label.new()


func _ready() -> void:
	theme = UiTheme.build()
	title = "Customize HUD"
	ok_button_text = "Apply"
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var main_row: HBoxContainer = HBoxContainer.new()
	main_row.add_theme_constant_override("separation", 16)
	var main_caption: Label = Label.new()
	main_caption.text = "Large figure"
	main_row.add_child(main_caption)
	_main.custom_minimum_size = Vector2(240, 0)
	main_row.add_child(_main)
	rows.add_child(main_row)
	rows.add_child(UiTheme.caption("Also show"))
	_grid.columns = 3
	_grid.add_theme_constant_override("h_separation", 24)
	rows.add_child(_grid)
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_hint.add_theme_font_size_override("font_size", 12)
	rows.add_child(_hint)
	add_child(rows)
	confirmed.connect(_on_confirmed)


## Opens the dialog with every metric (`TorqaApp.hud_metrics()`) and the current `layout`.
func edit(metrics: Array, layout: PackedStringArray) -> void:
	_main.clear()
	_order.clear()
	for child: Node in _grid.get_children():
		child.queue_free()
	_checks.clear()
	for metric: Dictionary in metrics:
		var id: String = metric["id"]
		var caption: String = metric["caption"]
		_order.append(id)
		_main.add_item(caption)
		var check: CheckBox = CheckBox.new()
		check.text = caption
		check.button_pressed = layout.slice(1).has(id)
		check.toggled.connect(func(_on: bool) -> void: _update_limits())
		_checks[id] = check
		_grid.add_child(check)
	_main.select(maxi(_order.find(layout[0] if not layout.is_empty() else "power"), 0))
	_update_limits()
	popup_centered()


## Greys out further metrics once the grid is full.
func _update_limits() -> void:
	var chosen: int = 0
	for check: CheckBox in _checks.values():
		chosen += 1 if check.button_pressed else 0
	for check: CheckBox in _checks.values():
		check.disabled = chosen >= MAX_SMALL and not check.button_pressed
	_hint.text = "%d of %d chosen. They appear in this order." % [chosen, MAX_SMALL]


func _on_confirmed() -> void:
	var main_id: String = _order[_main.selected]
	var layout: PackedStringArray = PackedStringArray([main_id])
	for id: String in _order:
		if id != main_id and _checks[id].button_pressed:
			layout.append(id)
	layout_confirmed.emit(layout)
