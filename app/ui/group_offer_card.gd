class_name GroupOfferCard
extends PanelContainer
# gdlint: disable=max-line-length
## The request to organize the group ride (#424), or to stand in should the organizer be lost
## (#426), shown to the chosen rider over the ride instead of only inside the group panel,
## where nobody would notice it: who asks, what organizing means and, when the rider's own
## discovery suggests friends could not reach them, that warning. It stays until answered, or
## until the organizer cancels or the request times out; the ride goes on meanwhile.

## The rider accepted organizer duties or standing in, or declined them.
signal answered(accept: bool)

const WIDTH: float = 520.0
## Room kept beside the card in a narrow window.
const SIDE: float = 24.0
const PADDING: float = 18.0

var _title: Label = UiTheme.value(20)
var _text: Label = Label.new()
var _warning: Label = Label.new()
var _accept: Button = Button.new()
var _decline: Button = Button.new()


func _init() -> void:
	hide()
	# Measured before their first layout, the wrapped lines stand a word to a line; an
	# anchored panel grows to that and would never shrink back by itself.
	minimum_size_changed.connect(_fit)
	var style: StyleBoxFlat = UiTheme.panel(0.94)
	style.border_color = Color(UiTheme.ACCENT, 0.7)
	style.set_border_width_all(0)
	style.border_width_top = 3
	style.set_content_margin_all(PADDING)
	add_theme_stylebox_override("panel", style)
	var box: VBoxContainer = VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	_title.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	# Says the organizer's name, which is never translated.
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	box.add_child(_title)
	_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_text.add_theme_color_override("font_color", UiTheme.MUTED)
	box.add_child(_text)
	_warning.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_warning.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_warning.add_theme_color_override("font_color", UiTheme.WARNING)
	box.add_child(_warning)
	var buttons: HBoxContainer = HBoxContainer.new()
	buttons.alignment = BoxContainer.ALIGNMENT_END
	buttons.add_theme_constant_override("separation", 8)
	_decline.text = tr("Decline")
	_decline.pressed.connect(_answer.bind(false))
	buttons.add_child(_decline)
	_accept.add_theme_stylebox_override("normal", UiTheme.accent_button())
	# The remote's first arrow reaches the answer without a mouse.
	_accept.add_to_group(RemoteNavigation.FIRST)
	_accept.pressed.connect(_answer.bind(true))
	buttons.add_child(_accept)
	box.add_child(buttons)
	add_child(box)


## Asks the rider to take over from `organizer`, or with `standby` to stand in should the
## organizer be lost, with `warning` about reaching them, if any.
func show_offer(organizer: String, warning: String, standby: bool = false) -> void:
	_title.text = (
		tr("%s asks you to stand in as organizer") % organizer
		if standby
		else tr("%s asks you to organize the ride") % organizer
	)
	_text.text = (
		tr(
			"Stand in as organizer if the organizer's connection is lost? After about a minute without them you would take over, so the group rides on. Accepting shares connection addresses with everyone in this group now, using your connection settings."
		)
		if standby
		else tr(
			"Would you like to organize this ride? You will control the workout and welcome new riders. Accepting shares connection addresses with everyone in this group, using your connection settings."
		)
	)
	_accept.text = tr("Stand in as organizer") if standby else tr("Accept organizer duties")
	_warning.text = warning
	_warning.visible = not warning.is_empty()
	# Wrapped labels are measured at the width they have: before the first layout, none.
	var width: float = minf(WIDTH, get_parent_area_size().x - 2.0 * SIDE)
	custom_minimum_size.x = width
	for label: Label in [_title, _text, _warning]:
		label.custom_minimum_size.x = width - 2.0 * PADDING
	show()
	_fit()


## As tall as its content, keeping its bottom where it is anchored.
func _fit() -> void:
	var bottom: float = position.y + size.y
	reset_size()
	position.y = bottom - size.y


func _answer(accept: bool) -> void:
	hide()
	answered.emit(accept)
