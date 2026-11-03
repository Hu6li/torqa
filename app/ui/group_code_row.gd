class_name GroupCodeRow
extends HBoxContainer
## A group code to send outside ORQWA (R195): shown read-only, so the rider sees what they
## send, with Copy, which says Copied for a moment once the code is on the clipboard, and its
## QR code for a phone to pass on.

## How long Copy says Copied.
const COPIED_S: float = 2.0

## The QR code window's title and the line below its code.
var qr_title: String = ""
var qr_hint: String = ""

var _field: LineEdit = LineEdit.new()
var _copy: Button = Button.new()
var _qr: Button = Button.new()
var _dialog: CodeQrDialog = CodeQrDialog.new()
var _copied_left: float = 0.0


func _ready() -> void:
	add_theme_constant_override("separation", 8)
	_field.editable = false
	_field.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_field.custom_minimum_size = Vector2(160, 40)
	add_child(_field)
	_copy.text = tr("Copy")
	_copy.custom_minimum_size.x = 96
	_copy.pressed.connect(_copy_code)
	add_child(_copy)
	_qr.text = tr("Show QR code")
	_qr.pressed.connect(
		func() -> void: _dialog.show_code(qr_title, OrqwaApp.qr_code(_field.text), qr_hint)
	)
	add_child(_qr)
	add_child(_dialog)
	set_process(false)
	show_code("")


## Shows `code`; with none, Copy and the QR code wait for one.
func show_code(code: String) -> void:
	if code != _field.text:
		_field.text = code
		_field.caret_column = 0
	_copy.disabled = code.is_empty()
	_qr.disabled = code.is_empty()


func _copy_code() -> void:
	DisplayServer.clipboard_set(_field.text)
	# Copied only when it is there: a refused clipboard must not look like success.
	if DisplayServer.clipboard_get() != _field.text:
		return
	_copy.text = tr("Copied")
	_copied_left = COPIED_S
	set_process(true)


func _process(delta: float) -> void:
	_copied_left -= delta
	if _copied_left <= 0.0:
		_copy.text = tr("Copy")
		set_process(false)
