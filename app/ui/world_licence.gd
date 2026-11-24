class_name WorldLicence
extends VBoxContainer
## A sold world on its course page (R293, ADR 0043). Locked: the licence key from the purchase
## and Activate. Activated on this device: who bought it, and Release this device, which frees
## this device in every paid world's licence. Logic stays in the core; this only asks and shows.

## The world was activated or released here: the course page shows it again.
signal changed

var _orqwa: OrqwaApp
var _path: String = ""
var _note: Label = Label.new()
var _key: LineEdit = LineEdit.new()
var _activate: Button = Button.new()
var _locked: VBoxContainer = VBoxContainer.new()
var _release: Button = Button.new()
var _confirm_release: ConfirmationDialog = ConfirmationDialog.new()
var _waiting: bool = false


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_orqwa.world_licence_changed.connect(_on_licence_changed)
	_orqwa.failed.connect(_on_failed)


## Shows the world of the course at `path` (`world` as `OrqwaApp.courses()` gives it); hidden
## for a world that is not sold, or a course.
func show_world(path: String, world: Dictionary) -> void:
	_path = path
	_waiting = false
	var locked: bool = world.get("locked", false)
	var licensee: String = world.get("licensee", "")
	visible = locked or not licensee.is_empty()
	_locked.visible = locked
	_release.visible = not locked and not licensee.is_empty()
	_key.text = ""
	_set_waiting(false)


## Whether the world waits for activation, so riding is not offered yet.
func locked() -> bool:
	return visible and _locked.visible


func _init() -> void:
	add_theme_constant_override("separation", 10)
	_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_note.text = tr(
		"This is a paid world. Enter the licence key from your purchase to ride it on this device."
	)
	_locked.add_theme_constant_override("separation", 10)
	_locked.add_child(_note)
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	_key.placeholder_text = tr("Licence key")
	_key.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_key.text_submitted.connect(func(_text: String) -> void: _on_activate())
	_key.text_changed.connect(func(_text: String) -> void: _set_waiting(_waiting))
	row.add_child(_key)
	_activate.text = tr("Activate")
	_activate.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_activate.pressed.connect(_on_activate)
	row.add_child(_activate)
	_locked.add_child(row)
	add_child(_locked)
	_release.text = tr("Release this device")
	_release.tooltip_text = tr("Frees this device in every paid world's licence; they lock here")
	_release.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	_release.pressed.connect(_confirm_release.popup_centered)
	add_child(_release)
	_confirm_release.title = tr("Release this device?")
	_confirm_release.dialog_text = (
		tr("All paid worlds lock on this device; each of their licences has a device free again.")
		+ " "
		+ tr("A licence can release only a few devices a year.")
	)
	_confirm_release.dialog_autowrap = true
	_confirm_release.ok_button_text = tr("Release")
	_confirm_release.confirmed.connect(_on_release)
	add_child(_confirm_release)


func _on_activate() -> void:
	var key: String = _key.text.strip_edges()
	if key.is_empty() or _waiting:
		return
	_set_waiting(true)
	_orqwa.activate_world(_path, key)


func _on_release() -> void:
	_set_waiting(true)
	_orqwa.release_world(_path)


func _set_waiting(waiting: bool) -> void:
	_waiting = waiting
	_activate.disabled = waiting or _key.text.strip_edges().is_empty()
	_key.editable = not waiting
	_release.disabled = waiting


func _on_licence_changed(path: String, _activated: bool) -> void:
	if path == _path:
		_set_waiting(false)
		changed.emit()


func _on_failed(_message: String) -> void:
	if _waiting:
		_set_waiting(false)
