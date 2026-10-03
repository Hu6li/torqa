class_name HudEditor
extends HBoxContainer
## Edits a HUD layout (R51, R54) right in the HUD: the HUD with example values, and the figures
## still available. Figures are dragged from the list straight to the place in the HUD where
## they should appear, moved around within the HUD and dragged back to the list to remove them;
## the + buttons add a figure at the end for those who prefer clicking.

signal layout_changed(layout: PackedStringArray)

const DRAG_KEY: String = HudPanel.DRAG_KEY

var _layout: PackedStringArray = PackedStringArray()
var _max: int = TorqaApp.hud_max_metrics()
var _available: VBoxContainer = VBoxContainer.new()
var _preview: HudPanel = HudPanel.new()
var _hint: Label = Label.new()


func _init() -> void:
	add_theme_constant_override("separation", 24)
	size_flags_vertical = Control.SIZE_EXPAND_FILL
	var hud_column: VBoxContainer = VBoxContainer.new()
	hud_column.add_theme_constant_override("separation", 8)
	hud_column.add_child(UiTheme.caption(tr("HUD — drop figures where you want them")))
	var frame: PanelContainer = PanelContainer.new()
	frame.custom_minimum_size = Vector2(280, 0)
	frame.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_preview.editable = true
	_preview.drop_requested.connect(place)
	frame.add_child(_preview)
	hud_column.add_child(frame)
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_hint.add_theme_font_size_override("font_size", 12)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_hint.custom_minimum_size = Vector2(280, 0)
	hud_column.add_child(_hint)
	add_child(hud_column)

	var list_column: VBoxContainer = VBoxContainer.new()
	list_column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	list_column.add_theme_constant_override("separation", 8)
	list_column.add_child(UiTheme.caption(tr("Available — drag in to add, drop here to remove")))
	var scroll: _DropArea = _DropArea.new(self)
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.custom_minimum_size = Vector2(240, 260)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_available.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_available.add_theme_constant_override("separation", 6)
	scroll.add_child(_available)
	list_column.add_child(scroll)
	add_child(list_column)


## Shows `layout` for editing, with units for an imperial or metric rider.
func edit(layout: PackedStringArray, imperial: bool) -> void:
	_preview.imperial = imperial
	_layout = layout.duplicate()
	_refresh()


## The edited layout, the large figure first.
func layout() -> PackedStringArray:
	return _layout.duplicate()


## Puts `id` at `index` in the HUD, moving it if it is shown already.
func place(id: String, index: int) -> void:
	var from: int = _layout.find(id)
	if from < 0 and _layout.size() >= _max:
		return
	if from >= 0:
		_layout.remove_at(from)
		if from < index:
			index -= 1
	_layout.insert(clampi(index, 0, _layout.size()), id)
	_changed()


## Takes `id` out of the HUD; the last figure stays, as a HUD needs at least one.
func remove(id: String) -> void:
	if _layout.size() > 1 and _layout.has(id):
		_layout.remove_at(_layout.find(id))
		_changed()


## The metric id carried by drag `data`, or "" if it is not a HUD figure.
static func dragged(data: Variant) -> String:
	return HudPanel.dragged(data)


func _changed() -> void:
	_refresh()
	layout_changed.emit(layout())


func _refresh() -> void:
	for child: Node in _available.get_children():
		_available.remove_child(child)
		child.queue_free()
	for id: String in _preview.catalogue:
		if not _layout.has(id):
			_available.add_child(_Chip.new(self, id, _caption(id)))
	_preview.show_layout(_layout)
	_preview.show_samples()
	_hint.text = (
		tr("%d of %d figures. The first one is shown large. Values are examples.")
		% [_layout.size(), _max]
	)


func _caption(id: String) -> String:
	var caption: String = _preview.catalogue[id]["caption"]
	return caption


## An available figure: drag it into the HUD, or add it at the end with +. Dropping a figure
## from the HUD on it removes that figure from the HUD.
class _Chip:
	extends PanelContainer

	var _editor: HudEditor
	var _id: String

	func _init(editor: HudEditor, id: String, caption: String) -> void:
		_editor = editor
		_id = id
		mouse_default_cursor_shape = Control.CURSOR_DRAG
		add_theme_stylebox_override("panel", UiTheme.chip(false))
		var row: HBoxContainer = HBoxContainer.new()
		var label: Label = Label.new()
		label.text = tr(caption)
		label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(label)
		var button: Button = Button.new()
		button.text = "+"
		button.flat = true
		button.focus_mode = Control.FOCUS_NONE
		button.tooltip_text = tr("Show")
		button.pressed.connect(func() -> void: _editor.place(_id, _editor.layout().size()))
		row.add_child(button)
		add_child(row)

	func _get_drag_data(_at: Vector2) -> Variant:
		var preview: Label = Label.new()
		preview.text = (get_child(0).get_child(0) as Label).text
		set_drag_preview(preview)
		return {DRAG_KEY: _id}

	func _can_drop_data(_at: Vector2, data: Variant) -> bool:
		return not HudEditor.dragged(data).is_empty()

	func _drop_data(_at: Vector2, data: Variant) -> void:
		_editor.remove(HudEditor.dragged(data))


## The list's free space: dropping a figure from the HUD there removes it.
class _DropArea:
	extends ScrollContainer

	var _editor: HudEditor

	func _init(editor: HudEditor) -> void:
		_editor = editor

	func _can_drop_data(_at: Vector2, data: Variant) -> bool:
		return not HudEditor.dragged(data).is_empty()

	func _drop_data(_at: Vector2, data: Variant) -> void:
		_editor.remove(HudEditor.dragged(data))
