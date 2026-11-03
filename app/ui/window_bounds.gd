class_name WindowBounds
extends Node
## Every application subwindow uses its embedder's interface coordinates (#485, R202).
## Installed by DialogWindows, including windows opened without UiScale.popup().

## Optional preferred size; automatic registration leaves a window's natural size intact.
var wanted: Vector2i = Vector2i.ZERO
var _window: Window
var _viewport: Viewport
var _minimum: Vector2i
var _maximum: Vector2i
var _last_minimum: Vector2i
var _last_maximum: Vector2i
var _updating: bool = false


## Shares one lifetime-bound guard between automatic registration and explicit popups.
static func attach(window: Window) -> WindowBounds:
	var bounds: WindowBounds = window.get_node_or_null("_OrqwaWindowBounds") as WindowBounds
	if bounds == null:
		bounds = WindowBounds.new()
		bounds.name = "_OrqwaWindowBounds"
		bounds._window = window
		window.add_child(bounds, false, Node.INTERNAL_MODE_BACK)
	return bounds


func _ready() -> void:
	_minimum = _window.min_size
	_maximum = _window.max_size
	_last_minimum = _minimum
	_last_maximum = _maximum
	# Godot embeds nested windows into the first ancestor that embeds subwindows, which
	# need not be their immediate parent. Bounds and positions must share that space.
	_viewport = _window.get_parent().get_viewport()
	while not _viewport.gui_embed_subwindows and _viewport.get_parent() != null:
		_viewport = _viewport.get_parent().get_viewport()
	_viewport.size_changed.connect(_parent_resized)
	_window.about_to_popup.connect(constrain)
	_window.visibility_changed.connect(_visibility_changed)
	_window.size_changed.connect(constrain)
	constrain()
	_visibility_changed()


func _visibility_changed() -> void:
	set_process(_window.visible)
	if _window.visible:
		constrain()


func _process(_delta: float) -> void:
	# Window has no position-changed signal for embedded dragging. This also catches
	# deferred content minima, translated messages and controls added while it is open.
	constrain()
	_scroll_overflow()


## Keeps limits in force even while Godot measures wrapped text at a temporary width.
func constrain() -> void:
	if _updating:
		return
	_updating = true
	var room: Vector2i = Vector2i(_viewport.get_visible_rect().size)
	var fits: Vector2i = (
		(room - Vector2i(2 * UiScale.DIALOG_MARGIN, _title() + 2 * UiScale.DIALOG_MARGIN))
		. max(Vector2i.ONE)
	)
	# Retain deliberate changes by callers, including new minima after a dialog opens.
	if _window.min_size != _last_minimum:
		_minimum = _window.min_size
	if _window.max_size != _last_maximum:
		_maximum = _window.max_size
	for axis: int in range(2):
		if _maximum[axis] > 0:
			fits[axis] = mini(fits[axis], _maximum[axis])
	_last_minimum = _minimum.min(fits)
	_last_maximum = fits
	# Godot ignores a maximum below an explicit minimum: lower the minimum first.
	_window.min_size = _last_minimum
	_window.max_size = _last_maximum
	_window.size = _window.size.min(fits)
	_keep_inside()
	_updating = false


## Applies the preferred size used by UiScale.popup; automatic guards preserve natural sizes.
func resize_to_wanted() -> void:
	constrain()
	_window.size = wanted.min(_last_maximum)
	_keep_inside()


## A centered client rectangle whose decorated frame also stays inside the embedder.
func centered_rect() -> Rect2i:
	var room: Vector2i = Vector2i(_viewport.get_visible_rect().size)
	return Rect2i((room - _window.size) / 2 + Vector2i(0, roundi(_title() / 2.0)), _window.size)


func _title() -> int:
	return 0 if _window.borderless else _window.get_theme_constant("title_height")


func _parent_resized() -> void:
	constrain()
	if _window.visible and wanted != Vector2i.ZERO:
		resize_to_wanted()


func _keep_inside() -> void:
	if not _window.visible:
		return
	var room: Vector2i = Vector2i(_viewport.get_visible_rect().size)
	var lower: Vector2i = Vector2i(UiScale.DIALOG_MARGIN, _title() + UiScale.DIALOG_MARGIN)
	var upper: Vector2i = (room - _window.size - Vector2i.ONE * UiScale.DIALOG_MARGIN).max(lower)
	_window.position = _window.position.clamp(lower, upper)


func _scroll_overflow() -> void:
	# AcceptDialog keeps its own action row outside these bodies. PopupMenu already
	# implements scrolling internally; its item controls are not application children.
	var children: Array[Node] = _window.get_children()
	if _window is AcceptDialog:
		var label: Label = (_window as AcceptDialog).get_label()
		if label.get_parent() == _window and not label.text.is_empty():
			children.append(label)
	if _window is FileDialog:
		var files: VBoxContainer = (_window as FileDialog).get_vbox()
		if files.get_parent() == _window and not children.has(files):
			children.append(files)
	for child: Node in children:
		if not child is Control:
			continue
		var content: Control = child as Control
		if not content.visible or content.is_set_as_top_level():
			continue
		var room: Vector2 = Vector2(_window.size)
		if _window is AcceptDialog:
			var buttons: Control = (_window as AcceptDialog).get_ok_button().get_parent() as Control
			room = buttons.position - content.position
			room.x = _window.size.x - 2.0 * content.position.x
		room = room.max(Vector2.ONE)
		if content is ScrollContainer:
			content.custom_minimum_size = content.custom_minimum_size.min(room)
			var existing: ScrollContainer = content as ScrollContainer
			var minimum: Vector2 = existing.get_combined_minimum_size()
			if minimum.x > room.x:
				existing.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
			if minimum.y > room.y:
				existing.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_RESERVE
			continue
		var need: Vector2 = content.get_combined_minimum_size()
		if need.x <= room.x and need.y <= room.y:
			continue
		var scroll: ScrollContainer = ScrollContainer.new()
		UiTheme.scroll_down_only(scroll)
		# Rigid content wider than the window also remains reachable; wrapped labels and
		# responsive containers retain the width they need for vertical reflow.
		if need.x > room.x:
			scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
		var at: int = _window.get_children().find(content)
		_window.add_child(scroll)
		if at >= 0:
			_window.move_child(scroll, at)
		content.reparent(scroll)
		content.set_anchors_and_offsets_preset(Control.PRESET_TOP_LEFT)
		content.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		content.size_flags_vertical = Control.SIZE_EXPAND_FILL
		if not _window is AcceptDialog:
			scroll.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
