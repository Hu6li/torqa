class_name WindowBounds
extends Node
## Every application subwindow uses its embedder's interface coordinates (#485, R202).
## Installed by DialogWindows, including windows opened without UiScale.popup().

## Optional initial size, enlarged when the content fits in the available room.
var wanted: Vector2i = Vector2i.ZERO
## Live panels can retain their opened size and scroll changing content instead (R212).
var fit_contents: bool = true
var _window: Window
var _viewport: Viewport
var _minimum: Vector2i
var _maximum: Vector2i
var _last_minimum: Vector2i
var _last_maximum: Vector2i
var _updating: bool = false
var _layout_pending: bool = true


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
	_window.size_changed.connect(_resized)
	constrain()
	_visibility_changed()


func _visibility_changed() -> void:
	set_process(_window.visible)
	if _window.visible:
		_layout_pending = true
		constrain()


func _process(_delta: float) -> void:
	# Window has no position-changed signal for embedded dragging. This also catches
	# deferred content minima, translated messages and controls added while it is open.
	constrain()
	# Width changes must reach wrapping containers before their height is measured.
	if _layout_pending:
		_layout_pending = false
		return
	var bodies: Array[Control] = _bodies()
	if not fit_contents or not _grow_to_contents(bodies):
		_scroll_overflow(bodies)


func _resized() -> void:
	_layout_pending = true
	constrain()


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


## Applies UiScale.popup's initial preference; shared content fitting may enlarge it.
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


func _bodies() -> Array[Control]:
	var children: Array[Node] = _window.get_children()
	if _window is AcceptDialog:
		var label: Label = (_window as AcceptDialog).get_label()
		if label.get_parent() == _window and not label.text.is_empty():
			children.append(label)
	if _window is FileDialog:
		var files: VBoxContainer = (_window as FileDialog).get_vbox()
		if files.get_parent() == _window and not children.has(files):
			children.append(files)
	var bodies: Array[Control] = []
	for child: Node in children:
		if child is Control:
			var content: Control = child as Control
			if content.visible and not content.is_set_as_top_level():
				bodies.append(content)
	return bodies


func _chrome(bodies: Array[Control]) -> Vector2:
	var minimum: Vector2 = Vector2.ZERO
	for body: Control in bodies:
		minimum = minimum.max(body.get_combined_minimum_size())
	# Window supplies its own panel margins and, for AcceptDialog, the fixed action row
	# and separation. Subtracting the bodies avoids duplicating each subclass's theme.
	return (_window.get_contents_minimum_size() - minimum).max(Vector2.ZERO)


func _natural_size(content: Control) -> Vector2:
	var need: Vector2 = content.get_combined_minimum_size()
	if content is ScrollContainer:
		var scroll: ScrollContainer = content as ScrollContainer
		var body: Vector2 = Vector2.ZERO
		for child: Node in scroll.get_children():
			if child is Control and (child as Control).visible:
				body = body.max((child as Control).get_combined_minimum_size())
		var margins: Vector2 = scroll.get_theme_stylebox("panel").get_minimum_size()
		if (
			scroll.vertical_scroll_mode == ScrollContainer.SCROLL_MODE_RESERVE
			or scroll.get_v_scroll_bar().visible
		):
			margins.x += (
				scroll.get_v_scroll_bar().get_combined_minimum_size().x
				+ scroll.get_theme_constant("scrollbar_h_separation")
			)
		if scroll.get_h_scroll_bar().visible:
			margins.y += (
				scroll.get_h_scroll_bar().get_combined_minimum_size().y
				+ scroll.get_theme_constant("scrollbar_v_separation")
			)
		need = need.max(body + margins)
	return need


func _grow_to_contents(bodies: Array[Control]) -> bool:
	var need: Vector2 = Vector2.ZERO
	for body: Control in bodies:
		need = need.max(_natural_size(body))
	need += _chrome(bodies)
	var size: Vector2i = _window.size
	if need.y > _last_maximum.y and size.x < _last_maximum.x:
		for body: Control in bodies:
			if _has_wrapped_lines(body):
				# A paragraph may fit at a wider reading width. Grow in bounded steps,
				# allowing a layout pass between them, rather than filling the whole screen.
				need.x = maxf(need.x, ceilf(size.x * 1.5))
				break
	# Use available width before asking wrapped content how much height it needs.
	for axis: int in range(2):
		var fits: int = mini(ceili(need[axis]), _last_maximum[axis])
		if fits > size[axis]:
			var center: Vector2 = Vector2(_window.position) + Vector2(size) / 2.0
			size[axis] = fits
			_window.size = size
			_window.position = Vector2i((center - Vector2(size) / 2.0).round())
			_keep_inside()
			return true
	return false


func _has_wrapped_lines(content: Control) -> bool:
	if not content.is_visible_in_tree():
		return false
	if content is Label:
		var label: Label = content as Label
		if (
			label.autowrap_mode != TextServer.AUTOWRAP_OFF
			and label.get_line_count() > label.text.count("\n") + 1
		):
			return true
	if content is RichTextLabel:
		var rich: RichTextLabel = content as RichTextLabel
		if (
			rich.autowrap_mode != TextServer.AUTOWRAP_OFF
			and rich.get_line_count() > rich.get_parsed_text().count("\n") + 1
		):
			return true
	for child: Node in content.get_children():
		if child is Control and _has_wrapped_lines(child as Control):
			return true
	return false


func _scroll_overflow(bodies: Array[Control]) -> void:
	# Only content that still cannot fit after growing needs scrolling. AcceptDialog
	# keeps its action row fixed; PopupMenu already scrolls its own internal items.
	var room: Vector2 = (Vector2(_window.size) - _chrome(bodies)).max(Vector2.ONE)
	for content: Control in bodies:
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
