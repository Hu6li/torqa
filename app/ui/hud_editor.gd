class_name HudEditor
extends HBoxContainer
## Edits a HUD layout (R51, R54): the chosen figures in order (the first is shown large), the
## HUD itself with example values, and the figures still available. Figures are dragged straight
## into the HUD to the place they should appear, moved within it and dragged out to remove
## them; the list and the + and × buttons do the same for those who prefer it.

signal layout_changed(layout: PackedStringArray)

const DRAG_KEY: String = HudPanel.DRAG_KEY

var _layout: PackedStringArray = PackedStringArray()
var _max: int = TorqaApp.hud_max_metrics()
var _chosen: VBoxContainer = VBoxContainer.new()
var _available: VBoxContainer = VBoxContainer.new()
var _preview: HudPanel = HudPanel.new()
var _hint: Label = Label.new()


func _init() -> void:
	add_theme_constant_override("separation", 20)
	size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(_column(tr("Shown — drag to reorder"), _chosen, true))
	var preview_column: VBoxContainer = VBoxContainer.new()
	preview_column.add_theme_constant_override("separation", 8)
	preview_column.add_child(UiTheme.caption(tr("HUD — drop figures where you want them")))
	var frame: PanelContainer = PanelContainer.new()
	frame.custom_minimum_size = Vector2(260, 0)
	frame.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_preview.editable = true
	_preview.drop_requested.connect(place)
	frame.add_child(_preview)
	preview_column.add_child(frame)
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_hint.add_theme_font_size_override("font_size", 12)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_hint.custom_minimum_size = Vector2(260, 0)
	preview_column.add_child(_hint)
	add_child(preview_column)
	add_child(_column(tr("Available — drag in to add"), _available, false))


## Shows `layout` for editing, with units for an imperial or metric rider.
func edit(layout: PackedStringArray, imperial: bool) -> void:
	_preview.imperial = imperial
	_layout = layout.duplicate()
	_refresh()


## The edited layout, the large figure first.
func layout() -> PackedStringArray:
	return _layout.duplicate()


## Puts `id` at `index` among the shown figures, moving it if it is shown already.
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


## Hides `id`; the last figure stays, as a HUD needs at least one.
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
	for list: VBoxContainer in [_chosen, _available]:
		for child: Node in list.get_children():
			list.remove_child(child)
			child.queue_free()
	for i: int in range(_layout.size()):
		_chosen.add_child(_Chip.new(self, _layout[i], _caption(_layout[i]), true, i == 0))
	for id: String in _preview.catalogue:
		if not _layout.has(id):
			_available.add_child(_Chip.new(self, id, _caption(id), false, false))
	_preview.show_layout(_layout)
	_preview.show_samples()
	_hint.text = (
		tr("%d of %d figures. The first one is shown large. Values are examples.")
		% [_layout.size(), _max]
	)


func _caption(id: String) -> String:
	var caption: String = _preview.catalogue[id]["caption"]
	return caption


func _column(title: String, list: VBoxContainer, chosen: bool) -> VBoxContainer:
	var column: VBoxContainer = VBoxContainer.new()
	column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	column.add_theme_constant_override("separation", 8)
	column.add_child(UiTheme.caption(title))
	var scroll: _DropArea = _DropArea.new(self, chosen)
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.custom_minimum_size = Vector2(220, 260)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	list.add_theme_constant_override("separation", 6)
	scroll.add_child(list)
	column.add_child(scroll)
	return column


## A figure in one of the lists; draggable, and a drop target for reordering.
class _Chip:
	extends PanelContainer

	var _editor: HudEditor
	var _id: String
	var _chosen: bool

	func _init(editor: HudEditor, id: String, caption: String, chosen: bool, large: bool) -> void:
		_editor = editor
		_id = id
		_chosen = chosen
		mouse_default_cursor_shape = Control.CURSOR_DRAG
		add_theme_stylebox_override("panel", UiTheme.chip(large))
		var row: HBoxContainer = HBoxContainer.new()
		var label: Label = Label.new()
		label.text = tr(caption)
		label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(label)
		if large:
			var badge: Label = UiTheme.caption(tr("Large"))
			row.add_child(badge)
		var button: Button = Button.new()
		button.text = "×" if chosen else "+"
		button.flat = true
		button.focus_mode = Control.FOCUS_NONE
		button.tooltip_text = tr("Hide") if chosen else tr("Show")
		button.pressed.connect(_on_button)
		row.add_child(button)
		add_child(row)

	func _on_button() -> void:
		if _chosen:
			_editor.remove(_id)
		else:
			_editor.place(_id, _editor.layout().size())

	func _get_drag_data(_at: Vector2) -> Variant:
		var preview: Label = Label.new()
		preview.text = (get_child(0).get_child(0) as Label).text
		set_drag_preview(preview)
		return {DRAG_KEY: _id}

	func _can_drop_data(_at: Vector2, data: Variant) -> bool:
		return not HudEditor.dragged(data).is_empty()

	func _drop_data(at: Vector2, data: Variant) -> void:
		var id: String = HudEditor.dragged(data)
		if not _chosen:
			_editor.remove(id)
			return
		# Dropped on the upper half: before this figure, else after it.
		var index: int = _editor.layout().find(_id) + (0 if at.y < size.y / 2.0 else 1)
		_editor.place(id, index)


## The empty space of a list: dropping there appends (shown) or hides (available).
class _DropArea:
	extends ScrollContainer

	var _editor: HudEditor
	var _chosen: bool

	func _init(editor: HudEditor, chosen: bool) -> void:
		_editor = editor
		_chosen = chosen

	func _can_drop_data(_at: Vector2, data: Variant) -> bool:
		return not HudEditor.dragged(data).is_empty()

	func _drop_data(_at: Vector2, data: Variant) -> void:
		var id: String = HudEditor.dragged(data)
		if _chosen:
			_editor.place(id, _editor.layout().size())
		else:
			_editor.remove(id)
