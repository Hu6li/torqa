class_name GroupListenCard
extends PanelContainer
## Asks again how the rider listens when voice moved to another output by itself, e.g.
## headphones plugged in (R194): over the ride, where the voice card is out of sight. The
## echo setting follows the answer; the ride goes on meanwhile.

## The rider listens on speakers, or else on headphones.
signal answered(speakers: bool)

const WIDTH: float = 460.0
## Room kept beside the card in a narrow window.
const SIDE: float = 24.0
const PADDING: float = 18.0

var _title: Label = UiTheme.value(20)
var _question: Label = Label.new()


func _init() -> void:
	hide()
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
	# Says the device's name, which is never translated.
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	box.add_child(_title)
	_question.text = tr("How are you listening?")
	_question.add_theme_color_override("font_color", UiTheme.MUTED)
	box.add_child(_question)
	var buttons: HBoxContainer = HBoxContainer.new()
	buttons.alignment = BoxContainer.ALIGNMENT_END
	buttons.add_theme_constant_override("separation", 8)
	for choice: Array in [
		[tr("Headphones"), tr("Everyone talks freely"), false],
		[tr("Speakers"), tr("One at a time"), true]
	]:
		var button: Button = Button.new()
		var label: String = choice[0]
		var meaning: String = choice[1]
		var speakers: bool = choice[2]
		button.text = label
		button.tooltip_text = meaning
		button.custom_minimum_size.x = 130
		button.pressed.connect(_answer.bind(speakers))
		buttons.add_child(button)
	box.add_child(buttons)
	add_child(box)


## Asks how the rider listens now that voice plays on `output`.
func ask(output: String) -> void:
	_title.text = tr("Voice now plays on %s") % output
	var width: float = minf(WIDTH, get_parent_area_size().x - 2.0 * SIDE)
	custom_minimum_size.x = width
	_title.custom_minimum_size.x = width - 2.0 * PADDING
	show()
	_fit()


## As tall as its content, keeping its bottom where it is anchored.
func _fit() -> void:
	var bottom: float = position.y + size.y
	reset_size()
	position.y = bottom - size.y


func _answer(speakers: bool) -> void:
	hide()
	answered.emit(speakers)
