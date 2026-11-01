class_name FailureDetails
extends VBoxContainer
## Keeps failures available after a notification fades, without interrupting the ride. It
## grows upward from where it is anchored to fit what it shows, scrolling past `MAX_HEIGHT`,
## on an opaque panel, so nothing underneath shows through or overlaps it.

const HISTORY_LIMIT: int = 20
## The tallest the list of failures grows before it scrolls.
const MAX_HEIGHT: float = 240.0
## The red edge that marks the card as an error, like the blue one under a moment's (R124).
const STRIPE: int = 4

## On its own at the window's corner (#436): a red card with a warning sign and the newest
## failure, so it reads as an error at a glance. Beside a status that already says what went
## wrong, as in the account dialog, only the buttons show.
var standalone: bool = false:
	set(value):
		standalone = value
		_show_card()

var _failures: Array[Dictionary] = []
var _toggle: Button = Button.new()
var _dismiss: Button = Button.new()
var _panel: PanelContainer = PanelContainer.new()
var _scroll: ScrollContainer = ScrollContainer.new()
var _rows: VBoxContainer = VBoxContainer.new()
var _card: PanelContainer = PanelContainer.new()
var _sign: TextureRect = TextureRect.new()
var _message: Label = Label.new()
var _earlier: Label = Label.new()
var _buttons: HBoxContainer = HBoxContainer.new()


func _init() -> void:
	alignment = BoxContainer.ALIGNMENT_END
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_theme_constant_override("separation", 8)
	var box: StyleBoxFlat = UiTheme.panel(1.0)
	box.content_margin_left = 16
	box.content_margin_right = 16
	box.content_margin_top = 12
	box.content_margin_bottom = 12
	_panel.add_theme_stylebox_override("panel", box)
	UiTheme.scroll_down_only(_scroll)
	_rows.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_rows.add_theme_constant_override("separation", 12)
	_scroll.add_child(_rows)
	_panel.add_child(_scroll)
	add_child(_panel)
	_panel.hide()
	_build_card()
	_show_card()
	hide()


func _build_card() -> void:
	var row: HBoxContainer = HBoxContainer.new()
	row.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_theme_constant_override("separation", 12)
	_sign.texture = UiIcons.texture("warning", 24, UiTheme.DANGER)
	_sign.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_sign.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	row.add_child(_sign)
	var column: VBoxContainer = VBoxContainer.new()
	column.mouse_filter = Control.MOUSE_FILTER_IGNORE
	column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	column.add_theme_constant_override("separation", 10)
	# Already in the rider's language, with file and device names left as they are.
	_message.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_message.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	column.add_child(_message)
	_buttons.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_buttons.alignment = BoxContainer.ALIGNMENT_END
	_earlier.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_earlier.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_earlier.add_theme_color_override("font_color", UiTheme.MUTED)
	_buttons.add_child(_earlier)
	_dismiss.text = tr("Dismiss")
	_dismiss.pressed.connect(clear)
	_buttons.add_child(_dismiss)
	_toggle.text = tr("Technical details")
	_toggle.toggle_mode = true
	_toggle.toggled.connect(_show_details)
	_buttons.add_child(_toggle)
	column.add_child(_buttons)
	row.add_child(column)
	_card.add_child(row)
	add_child(_card)


## The card's look on its own, or bare buttons that leave the room beside them to the page.
func _show_card() -> void:
	if standalone:
		var box: StyleBoxFlat = UiTheme.panel(1.0)
		box.border_color = UiTheme.DANGER
		box.set_border_width_all(0)
		box.border_width_left = STRIPE
		_card.add_theme_stylebox_override("panel", box)
	else:
		_card.add_theme_stylebox_override("panel", StyleBoxEmpty.new())
	_card.mouse_filter = Control.MOUSE_FILTER_STOP if standalone else Control.MOUSE_FILTER_IGNORE
	_buttons.size_flags_horizontal = (
		Control.SIZE_EXPAND_FILL if standalone else Control.SIZE_SHRINK_END
	)
	_sign.visible = standalone
	_show_newest()


func _show_newest() -> void:
	_message.visible = standalone and not _failures.is_empty()
	_message.text = summary(_failures[0]) if _message.visible else ""
	_earlier.visible = standalone and _failures.size() > 1
	_earlier.text = tr("+%d earlier") % (_failures.size() - 1) if _earlier.visible else ""


## Arguments are substituted after translation and remain untouched, even if they contain %s.
static func summary(failure: Dictionary) -> String:
	var message: String = failure.get("message", "An unexpected error occurred.")
	var values: PackedStringArray = failure.get("args", PackedStringArray())
	var parts: PackedStringArray = TranslationServer.translate(message).split("%s")
	var text: String = parts[0]
	for i: int in range(1, parts.size()):
		text += (values[i - 1] if i <= values.size() else "%s") + parts[i]
	return text


## Stores a snapshot of the key, arguments and original diagnostic, newest first.
func report(failure: Dictionary) -> void:
	if failure.is_empty():
		return
	_failures.push_front(failure.duplicate(true))
	if _failures.size() > HISTORY_LIMIT:
		_failures.resize(HISTORY_LIMIT)
	_refresh()
	show()


## Forgets the failures shown and hides until the next one.
func clear() -> void:
	_failures.clear()
	_toggle.button_pressed = false
	_refresh()
	hide()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_instance_valid(_toggle):
		_toggle.text = tr("Technical details")
		_refresh()


func _refresh() -> void:
	_show_newest()
	for child: Node in _rows.get_children():
		_rows.remove_child(child)
		child.queue_free()
	for failure: Dictionary in _failures:
		var explanation: Label = Label.new()
		explanation.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		explanation.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		explanation.text = summary(failure)
		_rows.add_child(explanation)
		var details: Label = Label.new()
		details.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		details.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		details.add_theme_color_override("font_color", UiTheme.MUTED)
		details.text = failure.get("details", "")
		# A diagnostic that only repeats the message in English says nothing more.
		var message: String = failure.get("message", "")
		details.visible = not _same(details.text, message)
		_rows.add_child(details)
	_fit.call_deferred()


func _show_details(on: bool) -> void:
	_panel.visible = on
	if on and is_inside_tree():
		# Fitted while hidden, the wrapped rows had no width and stood a word to a line. The
		# shown panel gives them one as it is laid out, by the next frame.
		await get_tree().process_frame
		_fit()


## The list as tall as its rows, up to `MAX_HEIGHT`. Wrapped text is as tall as the width it
## gets, which the layout settles a frame later: fitted again until it stays (a few times at
## most), never on every change, which would chase itself.
func _fit(tries: int = 3) -> void:
	var height: float = minf(_rows.get_combined_minimum_size().y, MAX_HEIGHT)
	if is_equal_approx(_scroll.custom_minimum_size.y, height) or tries <= 0:
		return
	_scroll.custom_minimum_size.y = height
	_fit.call_deferred(tries - 1)


## Whether two texts say the same, apart from case, spaces and punctuation.
static func _same(a: String, b: String) -> bool:
	var plain: Callable = func(text: String) -> String:
		var kept: String = ""
		for character: String in text.to_lower():
			if character.is_valid_identifier() or character.is_valid_int():
				kept += character
		return kept
	return plain.call(a) == plain.call(b)
