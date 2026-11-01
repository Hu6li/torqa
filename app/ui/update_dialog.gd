class_name UpdateDialog
extends ConfirmationDialog
## Offers a newer ORQWA (R99): on Windows it downloads the installer, closes ORQWA and installs
## it, ORQWA starting again when done; on Android ORQWA's Android plugin installs the APK and
## Android closes ORQWA (ADR 0024); elsewhere it opens the release page. Shown only outside
## rides; the rider's rides and settings stay as they are.

## ORQWA's Android plugin, there only on Android.
const ANDROID: StringName = &"OrqwaAndroid"

var _orqwa: OrqwaApp
var _version: String = ""
var _page: String = ""
var _installable: bool = false
## Android forbids every app to install apps here (Advanced Protection or the device's admin).
var _forbidden: bool = false
## The installer is being downloaded.
var _downloading: bool = false


func _init() -> void:
	# Not exclusive: the settings may be open when the rider asked for the check there.
	exclusive = false
	dialog_hide_on_ok = false
	dialog_autowrap = true
	min_size = Vector2i(520, 0)
	confirmed.connect(_on_confirmed)
	custom_action.connect(_on_custom_action)
	add_button(tr("What's new"), false, "notes")


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Update available")


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_orqwa.update_downloaded.connect(_on_downloaded)
	_orqwa.update_failed.connect(_on_failed)
	if Engine.has_singleton(ANDROID):
		Engine.get_singleton(ANDROID).connect(&"update_failed", _on_android_failed)


## Shows the newer `version`, its release `page`, and whether ORQWA can install it itself.
func offer(version: String, page: String, installable: bool) -> void:
	_version = version
	_page = page
	_forbidden = (
		installable
		and Engine.has_singleton(ANDROID)
		and not Engine.get_singleton(ANDROID).call(&"canInstallUpdates")
	)
	_installable = installable and not _forbidden
	_downloading = false
	_show_offer()
	UiScale.popup(self, Vector2i(560, 220))


func _show_offer() -> void:
	var text: String = tr("ORQWA %s is available; you have %s.") % [_version, OrqwaCore.version()]
	if _installable:
		if Engine.has_singleton(ANDROID):
			text += " " + tr("ORQWA downloads and installs it; then open ORQWA again.")
		else:
			text += " " + tr("ORQWA downloads it, closes and installs it, then starts again.")
		text += " " + tr("Your rides and settings stay as they are.")
		ok_button_text = tr("Update now")
	else:
		if _forbidden:
			# Advanced Protection allows no exception for one app: only turning it off helps.
			text += " " + tr("This device lets no app install apps, so ORQWA cannot update itself.")
			text += " " + tr("If Advanced Protection is on, turn it off in Android's settings.")
			text += " " + tr("It is under Security & privacy; then check for updates again.")
		else:
			text += " " + tr("Download it from its release page.")
		ok_button_text = tr("Open the release page")
	dialog_text = text
	cancel_button_text = tr("Later")
	get_ok_button().disabled = false


func _on_confirmed() -> void:
	if not _installable:
		OS.shell_open(_page)
		hide()
		return
	if _orqwa == null or not _orqwa.download_update():
		return
	_downloading = true
	get_ok_button().disabled = true
	dialog_text = tr("Downloading ORQWA %s…") % _version


func _on_custom_action(action: StringName) -> void:
	if action == &"notes":
		OS.shell_open(_page)


func _on_downloaded() -> void:
	if not _downloading:
		return
	dialog_text = tr("Installing ORQWA %s…") % _version
	if Engine.has_singleton(ANDROID):
		# Android's installer closes ORQWA itself once the update is in place.
		Engine.get_singleton(ANDROID).call(&"installUpdate", _orqwa.downloaded_update())
		return
	# The installer replaces ORQWA's files only once it has closed.
	if _orqwa.install_update():
		get_tree().quit()
		return
	_downloading = false
	_show_offer()


func _on_failed(message: String) -> void:
	if not _downloading:
		return
	_downloading = false
	_show_offer()
	dialog_text = message + "\n\n" + dialog_text


func _on_android_failed(details: String) -> void:
	_orqwa.android_update_failed(details)
