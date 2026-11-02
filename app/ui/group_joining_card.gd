class_name GroupJoiningCard
extends VBoxContainer
## Joining a friend's ride (R195), in two steps: send the reply to the organizer outside
## ORQWA, then wait while the reply's minute runs out to be added. Not added in time, the
## rider makes a new reply from the same invitation instead of asking for another.

## The rider gives up joining.
signal cancelled
## The rider wants a new reply, the last one having run out.
signal renewed

var _title: Label = Label.new()
var _send: Label = Label.new()
var _code: GroupCodeRow = GroupCodeRow.new()
var _added: Label = Label.new()
var _left: Label = Label.new()
var _renew: Button = Button.new()
var _waiting: HBoxContainer = HBoxContainer.new()
var _waiting_text: Label = Label.new()


func _ready() -> void:
	add_theme_constant_override("separation", 16)
	_title.add_theme_font_size_override("font_size", 22)
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	add_child(_title)
	_step("1", _send)
	_code.qr_title = tr("Your reply")
	add_child(_indented(_code))
	var second: HBoxContainer = _step("2", _added)
	_left.add_theme_color_override("font_color", UiTheme.MUTED)
	_left.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	second.add_child(_left)
	_renew.text = tr("New reply")
	_renew.custom_minimum_size = Vector2(140, 40)
	_renew.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_renew.pressed.connect(renewed.emit)
	second.add_child(_renew)
	_waiting.add_theme_constant_override("separation", 10)
	var spinner: Spinner = Spinner.new(18.0)
	spinner.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_waiting.add_child(spinner)
	_waiting_text.add_theme_color_override("font_color", UiTheme.MUTED)
	_waiting_text.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_waiting.add_child(_waiting_text)
	add_child(_indented(_waiting))
	var cancel: Button = Button.new()
	cancel.text = tr("Cancel")
	cancel.custom_minimum_size.x = 140
	cancel.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	cancel.pressed.connect(cancelled.emit)
	add_child(cancel)


func _step(number: String, text: Label) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	var badge: PanelContainer = UiTheme.initial(number, 24)
	row.add_child(badge)
	text.add_theme_font_size_override("font_size", 17)
	text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	text.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	text.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	row.add_child(text)
	add_child(row)
	return row


func _indented(control: Control) -> MarginContainer:
	var margin: MarginContainer = MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 34)
	margin.add_child(control)
	return margin


## Shows the ride of `organizer` being joined, the `reply` to send them, the `seconds` left
## for it, and whether it ran out (`expired`) before they added the rider.
func show_joining(organizer: String, reply: String, seconds: float, expired: bool) -> void:
	_title.text = tr("Joining %s's ride") % organizer
	_send.text = tr("Send this reply to %s by chat, email or text message") % organizer
	_code.qr_hint = tr("Scan with your phone, then send the text to %s.") % organizer
	_code.show_code("" if expired else reply)
	_added.text = (
		tr("%s didn't add you in time") % organizer if expired else tr("%s adds you") % organizer
	)
	_left.visible = not expired and seconds > 0.0
	var left: int = ceili(seconds)
	_left.text = "%d:%02d" % [floori(left / 60.0), left % 60]
	_renew.visible = expired
	_waiting.visible = not expired
	_waiting_text.text = tr("Waiting for %s…") % organizer
