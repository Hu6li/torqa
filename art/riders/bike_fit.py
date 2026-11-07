"""Road-frame dimensions and contact points, independent of the rider's arm reach.

The small/medium proportions use the published Aethos 49/54 geometry as a reference:
https://www.specialized.com/us/en/s-works-aethos-sram-red-etap-axs/p/216931
The unbranded mesh is authored by ORQWA, not derived from a manufacturer's model.
"""

import math

from mathutils import Vector


SIZES = {
    "female": dict(stack=0.514, reach=0.375, head_tube=0.109, seat_tube=0.431,
                   head_angle=71.75, seat_angle=75.5, fork_offset=0.047,
                   stem=0.08, bar_width=0.38),
    "male": dict(stack=0.544, reach=0.384, head_tube=0.137, seat_tube=0.481,
                 head_angle=73.0, seat_angle=74.0, fork_offset=0.044,
                 stem=0.10, bar_width=0.42),
}


def layout(sex, bottom_bracket, wheel_radius):
    """The frame, steering axis and cockpit in Blender's metre coordinates."""
    size = SIZES[sex]
    head_angle = math.radians(size["head_angle"])
    seat_angle = math.radians(size["seat_angle"])
    steer = Vector((0.0, math.cos(head_angle), -math.sin(head_angle)))
    forward = Vector((0.0, math.sin(head_angle), math.cos(head_angle)))
    seat_axis = Vector((0.0, -math.cos(seat_angle), math.sin(seat_angle)))
    head_top = bottom_bracket + Vector((0.0, size["reach"], size["stack"]))
    head_bottom = head_top + steer * size["head_tube"]
    # Offset is perpendicular to the steering axis, not an extra horizontal reach.
    fork_axis = (head_bottom.z - wheel_radius
                 + forward.z * size["fork_offset"]) / -steer.z
    front_hub = head_bottom + steer * fork_axis + forward * size["fork_offset"]
    front_hub.z = wheel_radius
    # A 10 mm headset/spacer stack and half of the 32 mm stem clamp.
    stem_rear = head_top - steer * 0.026
    stem_pitch = math.radians(6.0)
    bar = stem_rear + Vector((0.0, size["stem"] * math.cos(stem_pitch),
                             size["stem"] * math.sin(stem_pitch)))
    return dict(seat_axis=seat_axis, steer=steer, head_top=head_top, head_bottom=head_bottom,
                front_hub=front_hub, stem_rear=stem_rear, bar=bar,
                hoods=bar + Vector((0.0, 0.11, 0.025)), half_width=size["bar_width"] / 2,
                cluster=bottom_bracket + seat_axis * size["seat_tube"])
