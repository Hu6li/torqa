class_name FailureDetails
extends VBoxContainer
## Keeps failures available after a notification fades, without interrupting the ride. It
## grows upward from where it is anchored to fit what it shows, scrolling past `MAX_HEIGHT`,
## on an opaque panel, so nothing underneath shows through or overlaps it.

const HISTORY_LIMIT: int = 20
## The tallest the list of failures grows before it scrolls.
const MAX_HEIGHT: float = 240.0

var _failures: Array[Dictionary] = []
var _toggle: Button = Button.new()
var _dismiss: Button = Button.new()
var _panel: PanelContainer = PanelContainer.new()
var _scroll: ScrollContainer = ScrollContainer.new()
var _rows: VBoxContainer = VBoxContainer.new()


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
	var buttons: HBoxContainer = HBoxContainer.new()
	buttons.size_flags_horizontal = Control.SIZE_SHRINK_END
	_dismiss.text = tr("Dismiss")
	_dismiss.pressed.connect(clear)
	buttons.add_child(_dismiss)
	_toggle.text = tr("Technical details")
	_toggle.toggle_mode = true
	_toggle.toggled.connect(_panel.set_visible)
	buttons.add_child(_toggle)
	add_child(buttons)
	hide()


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
		_dismiss.text = tr("Dismiss")
		_refresh()


func _refresh() -> void:
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
