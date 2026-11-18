class_name GroupAwayChecks
extends RefCounted
## A friend away from the group (R249, R250): everyone sees how long; from a changed network,
## Join again rather than reconnecting; at the organizer each away friend's row has a field for
## their new reply instead of readiness and ping.


static func check(group: GroupRidesTab, away: Array, verify: Callable) -> void:
	# R249, #536: everyone sees how long; from a changed network reconnecting cannot work,
	# and a new reply can.
	away[1]["away_s"] = 42.5
	var moved: Dictionary = {
		"host": false,
		"phase": "riding",
		"reconnect_s": 272.4,
		"moved": true,
		"can_rejoin": true,
		"roster": away,
	}
	group._show_status(moved)
	var sam: Label = group._roster.get_child(1).get_child(1).get_child(1)
	verify.call(sam.text.contains("0:42"), "how long Sam is away: %s" % sam.text)
	verify.call(
		group._next_step._rejoin.visible and not group._next_step._reconnect.visible,
		"a changed network joins again rather than reconnecting"
	)
	verify.call(
		group._next_step._line.text.contains("network changed"), group._next_step._line.text
	)
	# R249: away friends' rows show no readiness or ping; the organizer pastes each one's new
	# reply right there, also with several away, and typing survives the list's rebuilds.
	var lobby: Array = [
		{"id": 0, "name": "Host", "ready": true, "me": true, "organizer": true},
		{"id": 1, "name": "Sam", "ready": true, "me": false, "away": true, "away_s": 42.5},
		{"id": 2, "name": "Kim", "ready": true, "me": false, "away": true, "away_s": 7.0},
		{"id": 3, "name": "Lou", "ready": false, "me": false, "ping_ms": 30},
	]
	var hosting: Dictionary = {
		"host": true, "phase": "waiting", "invite": "private", "roster": lobby
	}
	group._show_status(hosting)
	verify.call(
		group._away_replies.keys() == [1, 2],
		"a field for each away friend: %s" % [group._away_replies.keys()]
	)
	verify.call(not group._pings.has(1) and group._pings.has(3), "no ping for an away friend")
	var sam_row: HBoxContainer = group._roster.get_child(1)
	verify.call(group._away_replies[1].get_parent() == sam_row, "Sam's field is in Sam's row")
	var pills: int = 0
	for child: Node in sam_row.get_children():
		pills += 1 if child is PanelContainer and not child is GroupAwayReply else 0
	verify.call(pills == 1, "only Sam's initial, no readiness pill")
	group._away_replies[1]._field.text = "orqwa:half"
	lobby[1]["away_s"] = 43.5
	group._show_status(hosting)
	verify.call(
		group._away_replies[1]._field.text == "orqwa:half", "the field keeps what was typed"
	)
	var kim: Dictionary = lobby[2]
	kim.erase("away")
	group._show_status(hosting)
	verify.call(group._away_replies.keys() == [1], "Kim back: her field is gone")
