class_name GroupFallbackConsentDialog
extends ConfirmationDialog
# gdlint: disable=max-line-length
## A group consent request is visible in the lobby and survives the transition into a ride.

var _orqwa: OrqwaApp
var _poll: float = 0.0
var _request: String = ""
var _answered: String = ""


func _init() -> void:
	dialog_hide_on_ok = false
	dialog_autowrap = true
	exclusive = false
	confirmed.connect(_answer.bind(true))
	canceled.connect(_answer.bind(false))


func _ready() -> void:
	theme = UiTheme.build()


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _process(delta: float) -> void:
	_poll += delta
	if _orqwa != null and _poll >= 0.2:
		_poll = 0.0
		show_status(_orqwa.group_status())


func show_status(status: Dictionary) -> void:
	_request = status.get("fallback_consent", "")
	if status.get("host", false) or not status.get("standby_offer", false):
		_request = ""
	if _request.is_empty():
		_answered = ""
		hide()
		return
	if _answered == _request or status.get("reconnect_s", 0.0) > 0.0:
		hide()
		return
	var organizer: String = ""
	for rider: Dictionary in status.get("roster", []):
		if rider.get("organizer", false):
			organizer = rider["name"]
	title = tr("%s asks you to stand in as organizer") % organizer
	dialog_text = tr(
		"Stand in as organizer if the organizer's connection is lost during a ride? Your agreement lasts while you stay in this group. Accepting allows ORQWA to share connection addresses with everyone in this group when preparing the fallback, using your connection settings."
	)
	ok_button_text = tr("Stand in as organizer")
	cancel_button_text = tr("Decline")
	if not visible:
		UiScale.popup(self, Vector2i(560, 240))


func _answer(accept: bool) -> void:
	if _orqwa != null and _orqwa.group_handover_answer(accept):
		_answered = _request
		hide()
