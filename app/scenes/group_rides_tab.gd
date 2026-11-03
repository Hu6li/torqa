class_name GroupRidesTab
extends VBoxContainer
# gdlint: disable=max-line-length
## Rider-facing setup; membership, transfers and readiness belong to the Rust core. What is
## ridden on the left, the group on the right, the same for organizer and friends (R196); in
## the ride's window (`compact`), the riders on the left and voice and workout on the right.

signal devices_requested

## Riders in a group, the organizer included.
const MAX_RIDERS: int = 8
## The readiness pills are this wide, so they stand in one column.
const PILL_WIDTH: float = 132.0

var compact: bool = false
var _orqwa: OrqwaApp
var _setup: BoxContainer = BoxContainer.new()
var _body: BoxContainer = BoxContainer.new()
var _sidebar: VBoxContainer = VBoxContainer.new()
## The second column of the ride's window: voice and the shared workout.
var _side_right: VBoxContainer = VBoxContainer.new()
var _leave: Button
var _activity_panel: PanelContainer = PanelContainer.new()
var _picker: GroupActivityPicker = GroupActivityPicker.new()
var _stage: Label = Label.new()
var _drafting: CheckBox = CheckBox.new()
var _together: CheckBox = CheckBox.new()
var _rule_summary: Label = Label.new()
var _rules: Control
var _rules_text: Label = Label.new()
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
var _next_step: GroupNextStep = GroupNextStep.new()
var _next_separator: HSeparator
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
var _connection: GroupConnectionDialog = GroupConnectionDialog.new()
## The invitation last joined, for a new reply when the organizer did not add the rider in
## time (R195), and its offline preview: the organizer and where everyone rides (R197).
var _joined_invite: String = ""
var _joined: Dictionary = {}
var _status: Dictionary = {}
var _roster_key: String = ""
## Each rider's voice button, by rider ID: their microphone on this rider's own row, a
## friend's speaker on theirs, lit while they are heard speaking (R190, R194).
var _voice_buttons: Dictionary[int, Button] = {}
## This rider's own readiness pill before the start (R196), null while there is none.
var _ready_pill: Button
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
	_next_step.bind(orqwa)
	if is_node_ready():
		refresh()


func _ready() -> void:
	add_theme_constant_override("separation", 18)
	if not compact:
		var header: HBoxContainer = HBoxContainer.new()
		_label(header, tr("Ride with friends"), 26)
		_leave = _button(header, tr("Leave group"), func() -> void: _orqwa.group_leave())
		_leave.custom_minimum_size.x = 150
		_leave.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		add_child(header)
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
	for column: VBoxContainer in [_sidebar, _side_right]:
		column.add_theme_constant_override("separation", 14)
		column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		column.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
		_body.add_child(column)
	# In the ride's window, voice and the workout make a second column beside the riders.
	var right: VBoxContainer = _side_right if compact else _sidebar
	_build_invitation()
	_build_roster()
	_build_organizer()
	var audio: VBoxContainer = _card(right)
	_label(audio, tr("Voice chat"), 20)
	audio.add_child(_voice_controls)
	_build_workout_controls(right)
	_build_dialogs()
	resized.connect(_reflow)
	visibility_changed.connect(refresh)
	_reflow()
	refresh()


func _build_rules() -> void:
	var row: HFlowContainer = HFlowContainer.new()
	row.add_theme_constant_override("h_separation", 24)
	_drafting.text = tr("Drafting")
	var assisted: String = tr("Assisted rides count for training, but do not set speed records.")
	_drafting.tooltip_text = tr("Save effort when riding behind friends.") + "\n" + assisted
	_together.text = tr("Keep together")
	_together.tooltip_text = (
		tr("Help everyone stay with the organizer while pedaling.") + "\n" + assisted
	)
	for option: CheckBox in [_drafting, _together]:
		option.button_pressed = true
		option.toggled.connect(_change_rules)
		row.add_child(option)
	_picker.options.add_child(row)
	_rules = row
	# Friends read the rules; only the organizer sets them.
	_rules_text.add_theme_color_override("font_color", UiTheme.MUTED)
	_picker.options.add_child(_rules_text)


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
	_join.invitation_entered.connect(_join_ride)


## Joins as the organizer chose, which the invitation says: nothing to ask (R197).
func _join_ride(invitation: String) -> void:
	var preview: Dictionary = _orqwa.group_invitation(invitation)
	if preview.is_empty() or not _orqwa.group_join(invitation):
		(
			_join
			. show_error(
				tr(
					"ORQWA could not join with this invitation. See Technical details for more information."
				)
			)
		)
		return
	_joined = preview
	_joined_invite = invitation
	_join.clear()


## A new reply from the same invitation, the last one having run out (R195).
func _renew_reply() -> void:
	if _joined_invite.is_empty():
		return
	_orqwa.group_leave()
	_orqwa.group_join(_joined_invite)


func _build_connecting() -> void:
	add_child(_connecting)
	_card(_connecting).add_child(_joining)
	_connecting_gap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_connecting_gap.size_flags_stretch_ratio = 0.6
	_connecting.add_child(_connecting_gap)
	_joining.cancelled.connect(func() -> void: _orqwa.group_leave())
	_joining.renewed.connect(_renew_reply)


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
	_next_separator = HSeparator.new()
	card.add_child(_next_separator)
	_next_step.devices_requested.connect(devices_requested.emit)
	card.add_child(_next_step)


func _build_dialogs() -> void:
	add_child(_connection)
	_connection.started.connect(_join.clear)
	_organizer_picker.theme = UiTheme.build()
	_organizer_picker.title = tr("Choose a new organizer")
	_organizer_choices.add_theme_constant_override("separation", 12)
	_organizer_choices.custom_minimum_size.x = 380
	_organizer_picker.add_child(UiTheme.scrolling(_organizer_choices))
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
	UiScale.popup(_organizer_picker, Vector2i(520, 180 + 52 * _organizer_choices.get_child_count()))


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
	_organizer_status.text = ""
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
	_organizer_status.visible = not _organizer_status.text.is_empty()


func _build_workout_controls(parent: VBoxContainer) -> void:
	var card: VBoxContainer = _card(parent)
	_workout_panel = card.get_parent() as PanelContainer
	_label(card, tr("Group workout"), 20)
	_workout_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_workout_status.tooltip_text = tr(
		"Your intensity stays personal. Pause in the ride controls stops only your own ride."
	)
	_workout_status.mouse_filter = Control.MOUSE_FILTER_PASS
	card.add_child(_workout_status)
	var buttons: HFlowContainer = HFlowContainer.new()
	buttons.add_theme_constant_override("h_separation", 8)
	buttons.add_theme_constant_override("v_separation", 8)
	card.add_child(buttons)
	_workout_pause = _button(
		buttons,
		tr("Pause workout for everyone"),
		func() -> void:
			var paused: bool = _status.get("workout_paused", false)
			_orqwa.group_workout_pause(not paused)
	)
	_workout_next = _button(
		buttons, tr("Next step for everyone"), func() -> void: _orqwa.skip_step()
	)


func _reflow() -> void:
	_setup.vertical = size.x < 760
	_connecting_gap.visible = size.x >= 1100
	_body.vertical = size.x < 760
	_side_right.visible = compact


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
## evidence, not proof. A group on one home network has nobody elsewhere to warn about (R197),
## and a host who turned the lookup off with only home addresses chose that, so only offered
## duties mention it.
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
	var late: bool = _status.get("late", false)
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
	_rule_summary.visible = compact and phase in ["countdown", "riding"]
	_rules.visible = host and not underway and not late
	_rules_text.text = _rule_summary.text
	_rules_text.visible = not _rules.visible
	# Only what the columns below do not show already (R196).
	var captions: Dictionary = {
		"connecting": tr("Creating your ride…") if host else tr("Connecting to your friends…"),
		"countdown": tr("Starting in %s seconds…") % ceili(countdown),
		"ended": tr("Group connection ended. Your local ride continues."),
		"failed": tr("Could not connect. Leave the group and ask for a fresh invitation."),
	}
	_stage.text = captions.get(phase, "")
	if late and not underway:
		_stage.text = tr("Your friends are already riding. Get ready and join when you like.")
	if can_retry:
		_stage.text = ""
	# The connection to the organizer was lost and comes back by itself (#434).
	var reconnect: float = _status.get("reconnect_s", 0.0)
	if reconnect > 0.0:
		_stage.text = tr("Connection lost. Reconnecting… Your local ride continues.")
	_stage.visible = not _stage.text.is_empty()
	if _leave != null:
		_leave.visible = phase != "idle"
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
	var invited_by: String = _joined.get("organizer", "")
	var mode: String = _joined.get("mode", "")
	_joining.show_joining(invited_by, mode, reply, reply_s, expired)
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
	# Always there beside the group, also while friends wait for the organizer (R196).
	_activity_panel.visible = not compact
	var organizer: String = _organizer_name()
	_picker.set_organizer(organizer, name)
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
	var trainer: bool = _orqwa.trainer_connected()
	_next_step.show_status(_status, trainer, organizer, _own_percent(), not compact)
	_next_separator.visible = _next_step.visible
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
		+ str(_status.get("late", false))
		+ phase
		+ TranslationServer.get_locale()
	)
	if key != _roster_key:
		_roster_key = key
		_show_roster(phase, host)
	_show_voice_buttons()
	_show_ready_pill()


## Who organizes the ride: from the group once in it, else as the invitation named them.
func _organizer_name() -> String:
	for rider: Dictionary in _status.get("roster", []):
		if rider.get("organizer", false):
			var rider_name: String = rider.get("name", "")
			return rider_name
	var invited_by: String = _joined.get("organizer", "")
	return invited_by


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
	_ready_pill = null
	var before: bool = phase not in ["countdown", "riding", "ended"]
	var removable: bool = host and before
	# Readiness as a pill on every row, this rider's their button (R196); friends joining
	# late enter instead.
	var pills: bool = before and not _status.get("late", false)
	for rider: Dictionary in _status.get("roster", []):
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var rider_name: String = rider["name"]
		row.add_child(UiTheme.initial(rider_name, 36))
		var text: VBoxContainer = VBoxContainer.new()
		text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var label: Label = _label(text, rider_name, 17)
		label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		var state: Label = _label(text, _rider_state(rider, phase, pills), 13)
		var live: bool = (
			not pills
			and not rider.get("away", false)
			and rider["ready"]
			and (phase != "riding" or rider["me"] or rider.get("present", false))
		)
		state.add_theme_color_override("font_color", UiTheme.SUCCESS if live else UiTheme.MUTED)
		state.visible = not state.text.is_empty()
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
		if pills and me:
			_ready_pill = _button(row, "", func() -> void: _orqwa.group_ready(not _is_ready()))
			_ready_pill.focus_mode = Control.FOCUS_NONE
			_ready_pill.size_flags_vertical = Control.SIZE_SHRINK_CENTER
			_ready_pill.custom_minimum_size.x = PILL_WIDTH
			_ready_pill.add_theme_font_size_override("font_size", 14)
		elif pills:
			var ready: bool = rider["ready"]
			row.add_child(_pill(ready))
		if removable:
			var remove: Button = _button(row, "", func() -> void: _orqwa.group_remove(id))
			remove.icon = UiIcons.texture("bin")
			remove.tooltip_text = tr("Remove %s from the group") % rider_name
			remove.focus_mode = Control.FOCUS_NONE
			remove.size_flags_vertical = Control.SIZE_SHRINK_CENTER
			UiTheme.danger_button(remove)
			if me:
				# Nobody removes themselves; it keeps the pills in one column.
				remove.disabled = true
				remove.modulate.a = 0.0
				remove.mouse_filter = Control.MOUSE_FILTER_IGNORE
		_roster.add_child(row)
	_show_ready_pill()


## The line below a rider's name: before the start how far they have got (#419), unless the
## pills beside it say whether they are ready; while riding where they are and their effort;
## their seat held while they reconnect (#434); and who they are to the group.
func _rider_state(rider: Dictionary, phase: String, pills: bool) -> String:
	var parts: PackedStringArray = []
	var ready: bool = rider["ready"]
	var stage: String = rider.get("stage", "")
	var percent: int = rider.get("percent", 0)
	if phase == "riding" and rider.get("present", false):
		var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
		var gap: float = rider["gap_m"]
		parts.append(
			(
				tr("%s ft") % roundi(gap / HudPanel.METERS_PER_FOOT)
				if imperial
				else tr("%s m") % roundi(gap)
			)
		)
		parts.append(GroupRidersPanel.effort(rider))
	elif phase == "riding":
		parts.append(tr("No signal") if ready else tr("Getting ready"))
		if rider["me"]:
			parts[0] = tr("Paused") if _status.get("local_paused", false) else tr("Riding")
	elif not ready and stage == "downloading":
		parts.append(tr("Downloading · %d %%") % percent)
	elif not ready and stage == "preparing" and percent < 100:
		parts.append(tr("Building the world · %d %%") % percent)
	elif not pills:
		parts.append(tr("Ready") if ready else tr("Getting ready"))
	if rider.get("away", false):
		parts = PackedStringArray([tr("Reconnecting…", "group_rider")])
	if rider["me"] and not (phase == "riding" and rider.get("present", false)):
		parts.append(tr("You"))
	if rider.get("organizer", false):
		parts.append(tr("Organizer"))
	# Takes over if the organizer's connection is lost (#426).
	if rider.get("fallback", false):
		parts.append(tr("Fallback organizer"))
	return " · ".join(parts)


## A friend's readiness: green when ready, quiet while not.
func _pill(ready: bool) -> PanelContainer:
	var pill: PanelContainer = PanelContainer.new()
	pill.custom_minimum_size.x = PILL_WIDTH
	pill.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var color: Color = UiTheme.SUCCESS if ready else UiTheme.MUTED
	pill.add_theme_stylebox_override("panel", _pill_box(color, ready))
	var word: Label = Label.new()
	word.text = tr("Ready") if ready else tr("Not ready")
	word.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	word.add_theme_font_size_override("font_size", 14)
	word.add_theme_color_override("font_color", color)
	pill.add_child(word)
	return pill


static func _pill_box(color: Color, strong: bool) -> StyleBoxFlat:
	var box: StyleBoxFlat = StyleBoxFlat.new()
	box.bg_color = Color(color, 0.18 if strong else 0.08)
	box.border_color = Color(color, 0.6 if strong else 0.25)
	box.set_border_width_all(1)
	box.set_corner_radius_all(15)
	box.content_margin_left = 12
	box.content_margin_right = 12
	box.content_margin_top = 6
	box.content_margin_bottom = 6
	box.anti_aliasing = true
	return box


func _is_ready() -> bool:
	var ready: bool = _status.get("ready", false)
	return ready


## This rider's pill, their way to say they are ready: inviting while they can, green once
## they are (pressing it again takes it back), and saying why it waits while their ride is
## prepared or their trainer is missing.
func _show_ready_pill() -> void:
	if _ready_pill == null:
		return
	var ready: bool = _is_ready()
	var prepared: bool = _status.get("prepared", false)
	var trainer: bool = _orqwa.trainer_connected()
	_ready_pill.disabled = not ready and (not prepared or not trainer)
	_ready_pill.text = tr("Ready") if ready else tr("I'm ready")
	var box: StyleBoxFlat = _pill_box(UiTheme.SUCCESS, true)
	var font: Color = UiTheme.SUCCESS
	if not ready:
		box = _pill_box(UiTheme.ACCENT, true)
		box.bg_color = UiTheme.ACCENT
		font = UiTheme.TEXT
	var hover: StyleBoxFlat = box.duplicate()
	hover.bg_color = hover.bg_color.lightened(0.12)
	for state: String in ["normal", "pressed", "focus"]:
		_ready_pill.add_theme_stylebox_override(state, box)
	_ready_pill.add_theme_stylebox_override("hover", hover)
	# Waiting, it still looks like this rider's button, not like a friend's Not ready.
	_ready_pill.add_theme_stylebox_override("disabled", _pill_box(UiTheme.ACCENT, false))
	for state: String in ["font_color", "font_hover_color", "font_pressed_color"]:
		_ready_pill.add_theme_color_override(state, font)
	_ready_pill.add_theme_color_override("font_disabled_color", Color(UiTheme.ACCENT, 0.7))
	var tip: String = tr("Tell the group you're ready")
	if ready:
		tip = tr("You're ready. Press to take it back.")
	elif not prepared:
		tip = tr("Your ride is still being prepared")
	elif not trainer:
		tip = tr("Connect your trainer first")
	_ready_pill.tooltip_text = tip


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
