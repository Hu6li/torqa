class_name CodeQrDialog
extends AcceptDialog
## A group code as a QR code (R195), for a phone to pass it on through a messenger: the
## invitation an organizer sends, or the reply a friend sends back.

## The code is at most this many pixels wide, so the window fits small screens.
const MAX_SIDE: float = 460.0

var _code: TextureRect = TextureRect.new()
var _hint: Label = Label.new()


func _ready() -> void:
	theme = UiTheme.build()
	ok_button_text = tr("Close")
	var box: VBoxContainer = VBoxContainer.new()
	box.add_theme_constant_override("separation", 14)
	_code.stretch_mode = TextureRect.STRETCH_KEEP_CENTERED
	box.add_child(_code)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_hint.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_hint.custom_minimum_size.x = 420
	box.add_child(_hint)
	add_child(UiTheme.scrolling(box))


## Shows `code` (as `OrqwaApp.qr_code()` gives it) titled `heading`, with `hint` below it.
func show_code(heading: String, code: Dictionary, hint: String) -> void:
	title = heading
	var size: int = code.get("size", 0)
	var side: int = size + 2 * QrImage.QUIET_ZONE
	var pixels: int = clampi(floori(MAX_SIDE / maxi(side, 1)), 3, QrImage.MODULE_PIXELS)
	_code.texture = QrImage.texture(code, pixels)
	_hint.text = hint
	UiScale.popup(self, Vector2i(520, 560))
