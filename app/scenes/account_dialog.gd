class_name AccountDialog
extends ConfirmationDialog
## Connects a rider's account on a training platform (R29): asks for what the platform needs,
## e.g. an API key, says where to find it, and checks it with the platform before closing.
## Platforms the rider logs in to (Strava) open their login in the browser meanwhile.
## What went wrong stays in the dialog, to correct and try again.

## The rider's account on `platform` is connected.
signal account_connected(platform: String)

var _torqa: TorqaApp
var _rider: String = ""
var _platform: String = ""
var _platform_name: String = ""
var _fields: VBoxContainer = VBoxContainer.new()
## The text fields, by the key the platform gave them.
var _edits: Dictionary[String, LineEdit] = {}
var _help: Label = Label.new()
var _link: LinkButton = LinkButton.new()
var _status: Label = Label.new()
## The platform's login, to open again should the browser not have shown it.
var _login_link: LinkButton = LinkButton.new()
## Waiting for the platform to answer.
var _connecting: bool = false


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.account_connected.connect(_on_connected)
	_torqa.account_failed.connect(_on_failed)
	_torqa.open_url.connect(_on_open_url)


## Opens the dialog for rider `rider`'s account on the platform `account` describes (an entry
## of `TorqaApp.accounts()`).
func open(rider: String, account: Dictionary) -> void:
	_rider = rider
	_platform = account.get("platform", "")
	_platform_name = account.get("name", "")
	title = tr("Connect %s") % _platform_name
	for child: Node in _fields.get_children():
		_fields.remove_child(child)
		child.queue_free()
	_edits.clear()
	var fields: Array = account.get("fields", [])
	for field: Dictionary in fields:
		var key: String = field.get("key", "")
		var label: String = field.get("label", "")
		_fields.add_child(UiTheme.caption(tr(label)))
		var secret: bool = field.get("secret", false)
		var edit: LineEdit = LineEdit.new()
		edit.secret = secret
		edit.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		register_text_enter(edit)
		_fields.add_child(edit)
		_edits[key] = edit
	var help: String = account.get("help", "")
	_help.text = tr(help)
	_help.visible = not help.is_empty()
	var url: String = account.get("help_url", "")
	_link.text = url
	_link.uri = url
	_link.visible = not url.is_empty()
	_status.hide()
	_login_link.hide()
	_set_connecting(false)
	popup_centered()
	if not _edits.is_empty():
		var first: LineEdit = _edits.values()[0]
		first.grab_focus()


func _init() -> void:
	ok_button_text = tr("Connect")
	# Stays open until the platform has said yes, to show what went wrong otherwise.
	dialog_hide_on_ok = false
	min_size = Vector2i(520, 0)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 8)
	_fields.add_theme_constant_override("separation", 8)
	rows.add_child(_fields)
	# Wrapped texts need a width: without one they wrap at every word, and the dialog grows as
	# tall as the window.
	_help.custom_minimum_size = Vector2(480, 0)
	_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_help.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(_help)
	_link.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_link.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	rows.add_child(_link)
	# The platform's own words.
	_status.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_status.custom_minimum_size = Vector2(480, 0)
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	rows.add_child(_status)
	_login_link.text = tr("Open the login again")
	_login_link.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	rows.add_child(_login_link)
	add_child(rows)
	confirmed.connect(_on_confirmed)
	canceled.connect(_on_canceled)


func _ready() -> void:
	theme = UiTheme.build()


func _on_confirmed() -> void:
	if _connecting:
		return
	var entered: Dictionary = {}
	for key: String in _edits:
		var text: String = _edits[key].text.strip_edges()
		if text.is_empty():
			_show_status(tr("Fill in every field."), UiTheme.DANGER)
			return
		entered[key] = text
	_set_connecting(true)
	_show_status(tr("Checking…"), UiTheme.MUTED)
	# What the browser shows after a login.
	var connected_page: String = tr("Torqa is connected. You can close this page.")
	var failed_page: String = tr("Torqa could not connect. Go back to Torqa to try again.")
	if not _torqa.connect_account(_rider, _platform, entered, connected_page, failed_page):
		_set_connecting(false)
		_show_status(tr("Cannot connect."), UiTheme.DANGER)


## The platform's login (Strava): shown in the rider's browser while the dialog waits.
func _on_open_url(url: String) -> void:
	if not _connecting:
		return
	OS.shell_open(url)
	_show_status(
		(
			tr("Log in to %s in your browser and let Torqa upload your rides. Then come back here.")
			% _platform_name
		),
		UiTheme.MUTED
	)
	_login_link.uri = url
	_login_link.show()


func _on_canceled() -> void:
	if _connecting:
		_torqa.cancel_connect()
	_set_connecting(false)


func _on_connected(rider: String, platform: String, _label: String) -> void:
	if not _connecting or rider != _rider or platform != _platform:
		return
	_set_connecting(false)
	_login_link.hide()
	hide()
	account_connected.emit(platform)


func _on_failed(rider: String, platform: String, message: String) -> void:
	if not _connecting or rider != _rider or platform != _platform:
		return
	_set_connecting(false)
	_login_link.hide()
	_show_status(message, UiTheme.DANGER)


func _set_connecting(on: bool) -> void:
	_connecting = on
	get_ok_button().disabled = on
	for edit: LineEdit in _edits.values():
		edit.editable = not on


func _show_status(text: String, color: Color) -> void:
	_status.text = text
	_status.add_theme_color_override("font_color", color)
	_status.show()
