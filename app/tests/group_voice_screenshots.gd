class_name GroupVoiceScreenshots
extends RefCounted
## Screenshots of group voice for the screenshot run (scripts/screenshots.sh).


## Joining voice (R194): how the rider listens, the sound check, a refused microphone, the
## voice card with the riders' voice buttons, and being asked again once voice moved.
static func capture(tree: SceneTree, start: StartPage, group: GroupRidesTab, out: String) -> void:
	var root: Window = tree.root
	var audio: GroupVoiceControls = group._voice_controls
	var dialog: GroupVoiceDialog = audio._dialog
	var devices: Dictionary = {
		"input_device": "headset",
		"output_device": "headset",
		"devices": [{"id": "headset", "name": "USB Headset", "input": true, "output": true}]
	}
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		group.refresh()
		audio.update_status(devices)
		# Joining voice asks how the rider listens, then checks their sound (R194).
		dialog.ask_join()
		await tree.create_timer(0.4).timeout
		root.get_texture().get_image().save_png(
			out.path_join("group-voice-join-" + locale + ".png")
		)
		dialog._listen_with(false)
		dialog._show_microphone(true)
		var checking: Dictionary = devices.duplicate()
		checking.merge(
			{"microphone_test": true, "microphone_level": 0.65, "microphone_voice_detected": true}
		)
		audio.update_status(checking)
		await tree.create_timer(0.4).timeout
		print("saved voice check ", locale, ": ", dialog.size)
		root.get_texture().get_image().save_png(
			out.path_join("group-voice-check-" + locale + ".png")
		)
		audio.update_status(devices)
		audio._show_permission("denied")
		await tree.create_timer(0.3).timeout
		root.get_texture().get_image().save_png(
			out.path_join("group-mic-permission-" + locale + ".png")
		)
		audio._permission_hint.hide()
		dialog.permission_hint.hide()
		dialog.hide()
		# In voice: the voice card, and a voice button and a bin beside each rider.
		var talking: Dictionary = devices.duplicate()
		talking.merge(
			{
				"phase": "waiting",
				"host": true,
				"invite": "orqwa:invitation",
				"voice": true,
				"microphone": true,
				"echo_prevention": false,
				"roster":
				[
					{"id": 0, "name": "Rider", "ready": false, "me": true, "organizer": true},
					{"id": 1, "name": "Alex", "ready": true, "me": false, "speaking": true},
					{"id": 2, "name": "Mia", "ready": false, "me": false, "muted": true}
				]
			}
		)
		group._show_status(talking)
		await tree.create_timer(0.3).timeout
		root.get_texture().get_image().save_png(
			out.path_join("group-voice-riders-" + locale + ".png")
		)
		var scroll: ScrollContainer = start.get("_group_page")
		scroll.scroll_vertical = 100000
		await tree.create_timer(0.3).timeout
		root.get_texture().get_image().save_png(out.path_join("group-voice-" + locale + ".png"))
		# Voice moved to headphones plugged in: the rider is asked again (R194).
		talking.merge({"output_name": "AirPods Pro", "ask_listening": true})
		group._show_status(talking)
		await tree.create_timer(0.3).timeout
		root.get_texture().get_image().save_png(
			out.path_join("group-voice-moved-" + locale + ".png")
		)
		scroll.scroll_vertical = 0
		audio.update_status({})
