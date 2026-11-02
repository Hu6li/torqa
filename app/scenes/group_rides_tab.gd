class_name GroupRidesTab
extends VBoxContainer
# gdlint: disable=max-line-length
## Rider-facing setup; membership, transfers and readiness belong to the Rust core.

signal devices_requested

## Riders in a group, the organizer included.
const MAX_RIDERS: int = 8

var compact: bool = false
var _orqwa: OrqwaApp
var _setup: BoxContainer = BoxContainer.new()
var _body: BoxContainer = BoxContainer.new()
var _sidebar: VBoxContainer = VBoxContainer.new()
var _activity_panel: PanelContainer = PanelContainer.new()
var _picker: GroupActivityPicker = GroupActivityPicker.new()
var _stage: Label = Label.new()
var _drafting: CheckBox = CheckBox.new()
var _together: CheckBox = CheckBox.new()
var _rule_summary: Label = Label.new()
var _connecting: HBoxContainer = HBoxContainer.new()
## Keeps the joining steps readable on a wide window instead of stretched across it.
var _connecting_gap: Control = Control.new()
var _join: GroupJoinCard = GroupJoinCard.new()
var _joining: GroupJoiningCard = GroupJoiningCard.new()
var _invite: GroupInviteCard = GroupInviteCard.new()
var _roster: VBoxContainer = VBoxContainer.new()
## Friends whose reply the organizer added and who are still connecting (R195).
var _admitting: VBoxContainer = VBoxContainer.new()
var _add: GroupAddRow = GroupAddRow.new()
var _next: Label = Label.new()
## Turns while ORQWA connects, receives or prepares the ride and the rider can only wait (#420).
var _working: Spinner = Spinner.new(18.0)
var _ready_button: Button = Button.new()
var _start: Button = Button.new()
var _enter: Button = Button.new()
var _devices: Button
var _retry: Button
var _reconnect: Button
var _voice_controls: GroupVoiceControls = GroupVoiceControls.new()
var _invitation_panel: PanelContainer
var _workout_panel: PanelContainer
var _workout_status: Label = Label.new()
var _workout_pause: Button
var _workout_next: Button
var _organizer_panel: PanelContainer
var _organizer_status: Label = Label.new()
var _organizer_reach: Label = Label.new()
var _organizer_change: Button
var _fallback_change: Button
## Whether the organizer picker chooses a fallback rather than the next organizer (#426).
var _picking_fallback: bool = false
var _organizer_accept: Button
var _organizer_decline: Button
var _organizer_cancel: Button
var _organizer_picker: AcceptDialog = AcceptDialog.new()
var _organizer_choices: VBoxContainer = VBoxContainer.new()
var _progress: ProgressBar = ProgressBar.new()
var _bytes: Label = Label.new()
var _connection: GroupConnectionDialog = GroupConnectionDialog.new()
var _status: Dictionary = {}
var _roster_key: String = ""
## Each rider's voice button, by rider ID: their microphone on this rider's own row, a
## friend's speaker on theirs, lit while they are heard speaking (R190, R194).
var _voice_buttons: Dictionary[int, Button] = {}
var _activity_key: String = ""
## Names the organizer's preview the picker shows a friend (#418), "" for none.
var _preview_key: String = ""
var _elapsed: float = 0.0


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_picker.bind(orqwa)
	_voice_controls.bind(orqwa)
	_connection.bind(orqwa)
	_add.bind(orqwa)
	if is_node_ready():
		refresh()


func _ready() -> void:
	add_theme_constant_override("separation", 18)
	if not compact:
		_label(self, tr("Ride with friends"), 26)
	_stage.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_stage.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_stage)
	_rule_summary.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_rule_summary.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_rule_summary)
	_build_setup()
	_build_connecting()
	_body.add_theme_constant_override("separation", 24)
	_body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_body)
	_activity_panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_activity_panel.size_flags_stretch_ratio = 1.6
	_activity_panel.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_activity_panel.add_child(_picker)
	_build_rules()
	_picker.chosen.connect(_select_activity)
	_picker.browsed.connect(
		func(course: String, workout: String) -> void: _orqwa.group_preview(course, workout)
	)
	_body.add_child(_activity_panel)
	_sidebar.add_theme_constant_override("separation", 16)
	_sidebar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_sidebar.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_body.add_child(_sidebar)
	_build_invitation()
	_build_roster()
	_build_organizer()
	_build_workout_controls()
	var audio: VBoxContainer = _card(_sidebar)
	_label(audio, tr("Voice chat"), 20)
	audio.add_child(_voice_controls)
	_button(_sidebar, tr("Leave group"), func() -> void: _orqwa.group_leave())
	_build_dialogs()
	resized.connect(_reflow)
	visibility_changed.connect(refresh)
	_reflow()
	refresh()


func _build_rules() -> void:
	var row: HFlowContainer = HFlowContainer.new()
	row.add_theme_constant_override("h_separation", 24)
	_drafting.text = tr("Drafting")
	_drafting.tooltip_text = tr("Save effort when riding behind friends.")
	_together.text = tr("Keep together")
	_together.tooltip_text = tr("Help everyone stay with the organizer while pedaling.")
	for option: CheckBox in [_drafting, _together]:
		option.button_pressed = true
		option.toggled.connect(_change_rules)
		row.add_child(option)
	_picker.options.add_child(row)
	_muted(_picker.options, tr("Assisted rides count for training, but do not set speed records."))


func _change_rules(_on: bool) -> void:
	_orqwa.group_assistance(_drafting.button_pressed, _together.button_pressed)


func _build_setup() -> void:
	_setup.add_theme_constant_override("separation", 24)
	add_child(_setup)
	var host: VBoxContainer = _card(_setup)
	_label(host, tr("Start a group ride"), 22)
	var create: Button = _button(
		host, tr("Create a ride"), func() -> void: _connection.ask_create()
	)
	create.custom_minimum_size.y = 48
	create.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_card(_setup).add_child(_join)
	_join.invitation_entered.connect(
		func(invitation: String) -> void: _connection.ask_join(invitation)
	)


func _build_connecting() -> void:
	add_child(_connecting)
	_card(_connecting).add_child(_joining)
	_connecting_gap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_connecting_gap.size_flags_stretch_ratio = 0.6
	_connecting.add_child(_connecting_gap)
	_joining.cancelled.connect(func() -> void: _orqwa.group_leave())
	_joining.renewed.connect(func() -> void: _connection.renew())


func _build_invitation() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_invitation_panel = card.get_parent() as PanelContainer
	card.add_child(_invite)


func _build_roster() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_label(card, tr("Your group"), 20)
	_roster.add_theme_constant_override("separation", 12)
	card.add_child(_roster)
	_admitting.add_theme_constant_override("separation", 12)
	card.add_child(_admitting)
	card.add_child(_add)
	card.add_child(HSeparator.new())
	_next.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_next.add_theme_color_override("font_color", UiTheme.MUTED)
	_next.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var next_row: HBoxContainer = HBoxContainer.new()
	next_row.add_theme_constant_override("separation", 10)
	_working.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	next_row.add_child(_working)
	next_row.add_child(_next)
	card.add_child(next_row)
	_progress.show_percentage = false
	_progress.custom_minimum_size.y = 6
	card.add_child(_progress)
	_bytes.add_theme_color_override("font_color", UiTheme.MUTED)
	card.add_child(_bytes)
	_ready_button.text = tr("I'm ready")
	_ready_button.toggle_mode = true
	_ready_button.custom_minimum_size.y = 44
	_ready_button.toggled.connect(func(on: bool) -> void: _orqwa.group_ready(on))
	card.add_child(_ready_button)
	_start.text = tr("Start group ride")
	_start.custom_minimum_size.y = 52
	_start.add_theme_font_size_override("font_size", 20)
	_start.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_start.pressed.connect(func() -> void: _orqwa.group_start())
	card.add_child(_start)
	_enter.text = tr("Join ride now")
	_enter.custom_minimum_size.y = 52
	_enter.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_enter.pressed.connect(func() -> void: _orqwa.group_enter())
	card.add_child(_enter)
	_devices = _button(card, tr("Connect your trainer…"), func() -> void: devices_requested.emit())
	_retry = _button(card, tr("Retry download"), func() -> void: _orqwa.group_retry())
	_reconnect = _button(card, tr("Reconnect now"), func() -> void: _orqwa.group_reconnect())


func _build_dialogs() -> void:
	add_child(_connection)
	_connection.started.connect(_join.clear)
	_connection.join_cancelled.connect(_join.focus)
	_organizer_picker.theme = UiTheme.build()
	_organizer_picker.title = tr("Choose a new organizer")
	_organizer_choices.add_theme_constant_override("separation", 12)
	_organizer_choices.custom_minimum_size.x = 380
	_organizer_picker.add_child(_organizer_choices)
	add_child(_organizer_picker)


func _build_organizer() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_organizer_panel = card.get_parent() as PanelContainer
	_label(card, tr("Ride organizer"), 20)
	_organizer_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	card.add_child(_organizer_status)
	_organizer_reach.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_organizer_reach.add_theme_color_override("font_color", UiTheme.WARNING)
	card.add_child(_organizer_reach)
	_organizer_change = _button(card, tr("Change organizer…"), _choose_organizer.bind(false))
	_fallback_change = _button(card, tr("Choose fallback organizer…"), _choose_organizer.bind(true))
	_organizer_accept = _button(
		card, tr("Accept organizer duties"), func() -> void: _orqwa.group_handover_answer(true)
	)
	_organizer_accept.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_organizer_decline = _button(
		card, tr("Decline"), func() -> void: _orqwa.group_handover_answer(false)
	)
	_organizer_cancel = _button(
		card, tr("Cancel organizer change"), func() -> void: _orqwa.group_handover_cancel()
	)


func _choose_organizer(fallback: bool) -> void:
	_picking_fallback = fallback
	_organizer_picker.title = (
		tr("Choose a fallback organizer") if fallback else tr("Choose a new organizer")
	)
	for child: Node in _organizer_choices.get_children():
		_organizer_choices.remove_child(child)
		child.queue_free()
	_muted(
		_organizer_choices,
		(
			tr("Choose a friend who takes over if your connection is lost. They must accept.")
			if fallback
			else tr("Choose a friend who is ready. They must accept before anything changes.")
		)
	)
	for rider: Dictionary in _status.get("roster", []):
		if rider["me"] or not rider["ready"]:
			continue
		var id: int = rider["id"]
		var rider_name: String = rider["name"]
		var choice: Button = _button(
			_organizer_choices,
			rider_name,
			func() -> void:
				if _picking_fallback:
					_orqwa.group_fallback(id)
				else:
					_orqwa.group_handover(id)
				_organizer_picker.hide()
		)
		choice.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_organizer_picker.popup_centered()


func _show_organizer() -> void:
	var phase: String = _status.get("phase", "idle")
	var handover: String = _status.get("handover", "idle")
	var host: bool = _status.get("host", false)
	var standby: bool = _status.get("standby_offer", false)
	var offer: bool = _status.get("handover_offer", false) or standby
	_organizer_panel.visible = phase == "riding" and (host or handover != "idle" or standby)
	_organizer_change.visible = host and handover == "idle"
	_organizer_change.disabled = not _status.get("can_handover", false)
	_fallback_change.visible = host and handover == "idle"
	_fallback_change.disabled = not _status.get("can_choose_fallback", false)
	_organizer_accept.visible = offer
	_organizer_accept.text = (
		tr("Stand in as organizer") if standby else tr("Accept organizer duties")
	)
	_organizer_decline.visible = offer
	_organizer_cancel.visible = _status.get("can_cancel_handover", false) and not offer
	_organizer_status.text = tr(
		"You choose who organizes the ride. Your recording continues when you hand over."
	)
	if handover == "offered":
		_organizer_status.text = (
			tr("Waiting for %s to accept organizer duties.") % _status.get("successor_name", "")
		)
	if offer:
		_organizer_status.text = tr(
			"Would you like to organize this ride? You will control the workout and welcome new riders. Accepting shares connection addresses with everyone in this group, using your connection settings."
		)
	if standby:
		_organizer_status.text = tr(
			"Stand in as organizer if the organizer's connection is lost? After about a minute without them you would take over, so the group rides on. Accepting shares connection addresses with everyone in this group now, using your connection settings."
		)
	var reach: String = _status.get("reachability", "")
	_organizer_reach.text = reach_warning(reach, false) if offer else ""
	_organizer_reach.visible = not _organizer_reach.text.is_empty()
	if handover in ["preparing", "committing"]:
		_organizer_status.text = (
			tr(
				"Connecting everyone to %s. Keep riding; your recording continues. Voice reconnects with your microphone as it was."
			)
			% _status.get("successor_name", "")
		)


func _build_workout_controls() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_workout_panel = card.get_parent() as PanelContainer
	_label(card, tr("Group workout"), 20)
	_workout_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	card.add_child(_workout_status)
	_muted(
		card,
		tr("Your intensity stays personal. Pause in the ride controls stops only your own ride.")
	)
	_workout_pause = _button(
		card,
		tr("Pause workout for everyone"),
		func() -> void:
			var paused: bool = _status.get("workout_paused", false)
			_orqwa.group_workout_pause(not paused)
	)
	_workout_next = _button(card, tr("Next step for everyone"), func() -> void: _orqwa.skip_step())


func _reflow() -> void:
	_setup.vertical = size.x < 760
	_connecting_gap.visible = size.x >= 1100
	_body.vertical = size.x < 760 or compact


func _card(parent: Node) -> VBoxContainer:
	var panel: PanelContainer = PanelContainer.new()
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var content: VBoxContainer = VBoxContainer.new()
	content.add_theme_constant_override("separation", 14)
	panel.add_child(content)
	parent.add_child(panel)
	return content


## What `reach` (the status's `reachability`) means for a host, or for a rider offered organizer
## duties: only the cases where friends at other homes may not get through; discovery is
## evidence, not proof. A host limited to its home network chose that, so only offered duties
## mention it.
func reach_warning(reach: String, hosting: bool) -> String:
	var hosts: Dictionary[String, String] = {
		"strict":
		tr(
			"Friends at other homes probably cannot reach you: your connection looks like carrier-grade NAT or a similarly strict router. Let a friend at another home create the ride."
		),
		"ipv6":
		tr(
			"Friends with IPv6 internet can reach you; others may not. If someone cannot join, let a friend at another home create the ride."
		),
		"unknown":
		tr(
			"ORQWA could not tell whether friends at other homes can reach you. If nobody can join, let a friend at another home create the ride."
		),
	}
	var organizers: Dictionary[String, String] = {
		"strict":
		tr(
			"Friends at other homes probably could not reach you as organizer: your connection looks like carrier-grade NAT or a similarly strict router."
		),
		"ipv6": tr("As organizer, friends without IPv6 internet might not reach you."),
		"unknown":
		tr("ORQWA could not tell whether friends at other homes could reach you as organizer."),
		"home": tr("As organizer, only friends on your home network could reach you."),
	}
	return hosts.get(reach, "") if hosting else organizers.get(reach, "")


func _label(parent: Node, text: String, font_size: int = 15) -> Label:
	var label: Label = Label.new()
	label.text = text
	label.add_theme_font_size_override("font_size", font_size)
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	parent.add_child(label)
	return label


func _muted(parent: Node, text: String) -> void:
	_label(parent, text).add_theme_color_override("font_color", UiTheme.MUTED)


func _button(parent: Node, text: String, action: Callable) -> Button:
	var button: Button = Button.new()
	button.text = text
	button.pressed.connect(action)
	parent.add_child(button)
	return button


func refresh() -> void:
	if _orqwa == null or not is_node_ready():
		return
	_picker.refresh()
	_activity_key = ""
	_update()


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed >= 0.2 and is_visible_in_tree() and _orqwa != null:
		_elapsed = 0.0
		_update()


func _update() -> void:
	_show_status(_orqwa.group_status())


func _show_status(status: Dictionary) -> void:
	_status = status
	var phase: String = _status.get("phase", "idle")
	var host: bool = _status.get("host", false)
	var prepared: bool = _status.get("prepared", false)
	var late: bool = _status.get("late", false)
	var entering: bool = _status.get("entering", false)
	var ready: bool = _status.get("ready", false)
	var name: String = _status.get("name", "")
	var countdown: float = _status.get("countdown_s", 0.0)
	var can_retry: bool = _status.get("can_retry", false)
	var underway: bool = phase in ["countdown", "riding", "ended"]
	var drafting: bool = _status.get("drafting", true)
	var together: bool = _status.get("keep_together", true)
	_drafting.set_pressed_no_signal(drafting)
	_together.set_pressed_no_signal(together)
	_drafting.disabled = not host or phase not in ["waiting", "ready"] or late
	_together.disabled = _drafting.disabled
	_rule_summary.text = (
		(tr("Drafting on") if drafting else tr("Drafting off"))
		+ " · "
		+ (tr("Keep together on") if together else tr("Keep together off"))
	)
	_rule_summary.visible = (
		phase in ["countdown", "riding"]
		or (late and not name.is_empty())
		or (not host and not name.is_empty() and phase in ["downloading", "preparing"])
	)
	var captions: Dictionary = {
		"idle": tr("Choose who starts the ride. You'll be riding together in a few steps."),
		"connecting": tr("Creating your ride…") if host else tr("Connecting to your friends…"),
		"waiting":
		(
			tr("Invite your friends, then choose what to ride.")
			if host
			else tr("You're in! The organizer is choosing your ride.")
		),
		"downloading": tr("Getting your activity from the organizer…"),
		"preparing": tr("Preparing your ride…"),
		"ready": tr("Your ride is ready. Let's get everyone to the start."),
		"countdown": tr("Starting in %s seconds…") % ceili(countdown),
		"riding": tr("Group ride in progress"),
		"ended": tr("Group connection ended. Your local ride continues."),
		"failed": tr("Could not connect. Leave the group and ask for a fresh invitation."),
	}
	_stage.text = captions.get(phase, "")
	if late and not underway:
		_stage.text = tr("Your friends are already riding. Get ready and join when you like.")
	_setup.visible = phase == "idle" and not compact
	# Not added in time, the joiner makes a new reply on the same page (R195).
	var expired: bool = _status.get("reply_expired", false)
	_connecting.visible = (
		not host and not compact and (phase == "connecting" or (phase == "failed" and expired))
	)
	if expired:
		_stage.text = ""
	var reply: String = _status.get("reply", "")
	var reply_s: float = _status.get("reply_s", 0.0)
	_joining.show_joining(_connection.organizer(), reply, reply_s, expired)
	_body.visible = phase != "idle" and not _connecting.visible
	var inviting: bool = host and phase not in ["ended", "failed"]
	_invitation_panel.visible = inviting
	var reach: String = _status.get("reachability", "")
	var invite: String = _status.get("invite", "")
	var handing_over: bool = _status.get("handover", "idle") != "idle"
	_invite.show_invite("" if handing_over else invite, reach_warning(reach, true))
	_show_admissions(inviting and not handing_over)
	# What the organizer is choosing, for friends to talk over before it is shared (#418).
	var preview_key: String = _status.get("preview", "")
	if preview_key != _preview_key:
		_preview_key = preview_key
		_picker.show_preview({} if preview_key.is_empty() else _orqwa.group_preview_data())
	_activity_panel.visible = (
		not compact and (host or (not name.is_empty() and prepared) or _picker.previewing())
	)
	var course_path: String = _status.get("course_path", "")
	var workout_id: String = _status.get("workout_id", "")
	var activity_failed: bool = _status.get("activity_failed", false)
	var activity_key: String = (
		course_path
		+ workout_id
		+ str(host)
		+ str(underway)
		+ str(activity_failed)
		+ TranslationServer.get_locale()
	)
	if activity_key != _activity_key:
		_activity_key = activity_key
		_picker.refresh()
		_picker.show_selection(
			host and not underway and not late,
			"" if activity_failed else course_path,
			"" if activity_failed else workout_id
		)
	var received: float = _status.get("received", 0)
	var total: float = _status.get("total", 0)
	_progress.visible = phase == "downloading" and total > 0
	_bytes.visible = _progress.visible
	_progress.max_value = maxf(1, total)
	_progress.value = received
	_bytes.text = tr("%.1f / %.1f MiB") % [received / 1048576, total / 1048576]
	# Building the course's world takes longest; the group hears how far it has got too.
	var built: int = _own_percent() if phase == "preparing" else 0
	if built > 0:
		_progress.visible = true
		_bytes.visible = true
		_progress.max_value = 100
		_progress.value = built
		_bytes.text = tr("Building the world · %d %%") % built
	_working.visible = phase in ["connecting", "downloading", "preparing"] and not can_retry
	var trainer: bool = _orqwa.trainer_connected()
	_next.text = (
		tr("Choose an activity to get started.")
		if host
		else tr("Waiting for the organizer to choose an activity.")
	)
	if not name.is_empty():
		_next.text = tr("Getting your ride ready…")
	if prepared:
		_next.text = (
			tr("Connect your trainer, then tell the group you're ready.")
			if not trainer
			else tr("All set? Let your friends know you're ready.")
		)
	if ready:
		_next.text = (
			tr("You're ready. Waiting for the rest of the group.")
			if host
			else tr("You're ready. The organizer will start the ride.")
		)
	if _status.get("can_start", false):
		_next.text = tr("Everyone is ready. Start when you like!")
	if underway:
		_next.text = _stage.text
	elif can_retry:
		_next.text = tr("Download stopped. Try again when you're ready.")
		_stage.text = _next.text
	if late and not underway and prepared:
		_next.text = (
			tr("Connect your trainer to join your friends.")
			if not trainer
			else tr(
				"Join near the organizer at the current workout step. Your recording starts now."
			)
		)
	if entering:
		_next.text = tr("Joining your friends…")
	# The connection to the organizer was lost and comes back by itself (#434).
	var reconnect: float = _status.get("reconnect_s", 0.0)
	if reconnect > 0.0:
		_stage.text = tr("Connection lost. Reconnecting… Your local ride continues.")
		_next.text = (
			tr("Trying for another %s. You keep your place in the group.")
			% ("%d:%02d" % [floori(reconnect / 60.0), floori(reconnect) % 60])
		)
		_working.visible = true
	_reconnect.visible = reconnect > 0.0
	_enter.visible = late and not underway
	_enter.disabled = not trainer or not _status.get("can_enter", false)
	_ready_button.visible = not underway and not late
	_ready_button.disabled = not prepared or (not trainer and not ready)
	_ready_button.set_pressed_no_signal(ready)
	_ready_button.text = tr("Ready ✓ · click to undo") if ready else tr("I'm ready")
	_start.visible = host and not underway and not compact
	_start.disabled = not _status.get("can_start", false)
	_devices.visible = not compact and not underway and not trainer
	_retry.visible = can_retry
	_voice_controls.update_status(_status)
	_show_workout_controls()
	_show_organizer()
	var riders: Array[Dictionary] = []
	for rider: Dictionary in _status.get("roster", []):
		var listed: Dictionary = rider.duplicate()
		# Who speaks changes several times a second; the voice buttons follow without
		# rebuilding the rows, and their buttons, under the rider's pointer.
		listed.erase("speaking")
		listed.erase("muted")
		riders.append(listed)
	var key: String = (
		str(riders)
		+ str(_status.get("voice", false))
		+ str(_status.get("local_paused", false))
		+ phase
		+ TranslationServer.get_locale()
	)
	if key != _roster_key:
		_roster_key = key
		_show_roster(phase, host)
	_show_voice_buttons()


## This rider's preparation as the group heard it (#419), 0 before the first report.
func _own_percent() -> int:
	for rider: Dictionary in _status.get("roster", []):
		if rider.get("me", false):
			var percent: int = rider.get("percent", 0)
			return percent
	return 0


func _show_workout_controls() -> void:
	_workout_panel.visible = _status.get("workout_shared", false)
	var paused: bool = _status.get("workout_paused", false)
	_workout_status.text = (
		tr("Workout paused for the group. You can pedal freely.")
		if paused
		else tr("The organizer controls workout timing for everyone.")
	)
	if _status.get("workout_complete", false):
		_workout_status.text = tr("Group workout complete")
	_workout_pause.text = (
		tr("Resume workout for everyone") if paused else tr("Pause workout for everyone")
	)
	_workout_pause.visible = _status.get("host", false)
	_workout_next.visible = _workout_pause.visible
	_workout_pause.disabled = not _status.get("can_control_workout", false)
	_workout_next.disabled = _workout_pause.disabled


func _show_roster(phase: String, host: bool) -> void:
	for child: Node in _roster.get_children():
		_roster.remove_child(child)
		child.queue_free()
	_voice_buttons.clear()
	var removable: bool = host and phase not in ["countdown", "riding", "ended"]
	for rider: Dictionary in _status.get("roster", []):
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var rider_name: String = rider["name"]
		row.add_child(UiTheme.initial(rider_name, 36))
		var text: VBoxContainer = VBoxContainer.new()
		text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var label: Label = _label(text, rider_name, 17)
		label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		var state: Label = _label(text, tr("Ready") if rider["ready"] else tr("Getting ready"), 13)
		# How far each friend has got (#419), until they are ready.
		var stage: String = rider.get("stage", "")
		var percent: int = rider.get("percent", 0)
		if not rider["ready"] and stage == "downloading":
			state.text = tr("Downloading · %d %%") % percent
		elif not rider["ready"] and stage == "preparing" and percent < 100:
			state.text = tr("Building the world · %d %%") % percent
		if phase == "riding":
			state.text = tr("No signal") if rider["ready"] else tr("Getting ready")
			if rider["me"]:
				state.text = tr("Paused") if _status.get("local_paused", false) else tr("Riding")
		# Their seat is held while their connection comes back (#434).
		var away: bool = rider.get("away", false)
		if away:
			state.text = tr("Reconnecting…", "group_rider")
		var live: bool = (
			not away
			and rider["ready"]
			and (phase != "riding" or rider["me"] or rider.get("present", false))
		)
		state.add_theme_color_override("font_color", UiTheme.SUCCESS if live else UiTheme.MUTED)
		if rider["me"]:
			state.text += " · " + tr("You")
		if phase == "riding" and rider.get("present", false):
			var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
			var gap: float = rider["gap_m"]
			state.text = (
				tr("%s ft") % roundi(gap / HudPanel.METERS_PER_FOOT)
				if imperial
				else tr("%s m") % roundi(gap)
			)
			state.text += " · " + GroupRidersPanel.effort(rider)
		if rider.get("organizer", false):
			state.text += " · " + tr("Organizer")
		# Takes over if the organizer's connection is lost (#426).
		if rider.get("fallback", false):
			state.text += " · " + tr("Fallback organizer")
		row.add_child(text)
		var id: int = rider["id"]
		var me: bool = rider["me"]
		if _status.get("voice", false):
			var voice: Button = Button.new()
			voice.focus_mode = Control.FOCUS_NONE
			voice.size_flags_vertical = Control.SIZE_SHRINK_CENTER
			voice.pressed.connect(_voice_pressed.bind(id, me))
			row.add_child(voice)
			_voice_buttons[id] = voice
		if removable:
			var remove: Button = _button(row, "", func() -> void: _orqwa.group_remove(id))
			remove.icon = UiIcons.texture("bin")
			remove.tooltip_text = tr("Remove %s from the group") % rider_name
			remove.focus_mode = Control.FOCUS_NONE
			remove.size_flags_vertical = Control.SIZE_SHRINK_CENTER
			UiTheme.danger_button(remove)
			if me:
				# Nobody removes themselves; it keeps the voice buttons in one column.
				remove.disabled = true
				remove.modulate.a = 0.0
				remove.mouse_filter = Control.MOUSE_FILTER_IGNORE
		_roster.add_child(row)


## Each voice button as the rider's voice is now: this rider's microphone on or off, a friend
## muted here or heard speaking.
func _show_voice_buttons() -> void:
	for rider: Dictionary in _status.get("roster", []):
		var id: int = rider["id"]
		if not _voice_buttons.has(id):
			continue
		var button: Button = _voice_buttons[id]
		var speaking: bool = rider.get("speaking", false)
		var icon: String = "speaker"
		var color: Color = UiTheme.SUCCESS if speaking else UiTheme.MUTED
		var rider_name: String = rider["name"]
		if rider["me"]:
			var on: bool = _status.get("microphone", false)
			icon = "microphone" if on else "microphone_off"
			if not on:
				color = UiTheme.DANGER
			elif not speaking:
				color = UiTheme.TEXT
			button.tooltip_text = (
				tr("Turn your microphone off") if on else tr("Turn your microphone on")
			)
			button.disabled = _status.get("deafened", false)
		elif rider.get("muted", false):
			icon = "speaker_off"
			color = UiTheme.DANGER
			button.tooltip_text = tr("Unmute %s") % rider_name
		else:
			button.tooltip_text = tr("Mute %s for you") % rider_name
		button.icon = UiIcons.texture(icon, 20, color)


func _voice_pressed(id: int, me: bool) -> void:
	if me:
		_voice_controls.toggle_microphone()
		return
	for rider: Dictionary in _status.get("roster", []):
		if rider["id"] == id:
			var muted: bool = rider.get("muted", false)
			_orqwa.group_voice_mute(id, not muted)


## Switches this rider's microphone on or off, e.g. from the ride's bar.
func toggle_microphone() -> void:
	# The ride's panel may be closed, its status not followed meanwhile.
	_voice_controls.update_status(_orqwa.group_status())
	_voice_controls.toggle_microphone()


func _select_activity(course: String, workout: String) -> void:
	_orqwa.group_select(course, workout)
	_activity_key = ""
	_update()


## Friends still connecting after the organizer added their reply, then the seat for the next
## one while the ride has room (R195).
func _show_admissions(inviting: bool) -> void:
	var admitting: int = _status.get("admitting", 0) if inviting else 0
	while _admitting.get_child_count() > admitting:
		var row: Node = _admitting.get_child(_admitting.get_child_count() - 1)
		_admitting.remove_child(row)
		row.queue_free()
	while _admitting.get_child_count() < admitting:
		_admitting.add_child(_connecting_row())
	var roster: Array = _status.get("roster", [])
	_add.visible = inviting and roster.size() + admitting < MAX_RIDERS
	var refusal: String = _status.get("refusal", "")
	var refusals: int = _status.get("refusals", 0)
	_add.show_refusal(refusal, refusals)


func _connecting_row() -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	var slot: CenterContainer = CenterContainer.new()
	slot.custom_minimum_size = Vector2(36, 36)
	slot.add_child(Spinner.new(26.0))
	row.add_child(slot)
	var label: Label = _label(row, tr("Connecting…"), 17)
	label.add_theme_color_override("font_color", UiTheme.MUTED)
	label.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	return row
