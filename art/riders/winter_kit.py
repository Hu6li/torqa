"""Cold-weather kit on the existing weighted body; the fit and grip stay shared."""

import math

import bpy
import bmesh
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def cap(gear, head, eyes, tree):
    """A thin thermal skullcap under the helmet, covering the ears and nape."""
    points = [head.matrix_world @ v.co for v in head.data.vertices]
    top = max(p.z for p in points)
    crown = [p for p in points if p.z > top - 0.005]
    centre = sum(crown, Vector()) / len(crown)
    centre.z = 0
    segments, rows = 48, 16
    outer, inner = [], []
    for row in range(rows):
        t = row / rows
        radii, heights, directions = [], [], []
        for k in range(segments):
            angle = math.tau * k / segments
            direction = Vector((math.sin(angle), math.cos(angle), 0))
            bottom = eyes - 0.07 + 0.10 * max(0.0, direction.y) ** 2
            z = bottom * (1 - t) + top * t
            start = centre + Vector((0, 0, z))
            hit = tree.ray_cast(start + direction * 0.4, -direction)[0]
            if hit is None:
                raise RuntimeError(f"thermal cap missed the head at ring {row}, sample {k}, z {z}")
            radii.append((hit - start).length)
            heights.append(z)
            directions.append(direction)
        # A fabric cover bridges the ear's hollows rather than reproducing its anatomy.
        radius = [max(radii[(k + offset) % segments] for offset in range(-2, 3))
                  for k in range(segments)]
        padding = [0.0045 + 0.009 * max(0.0, min(1.0, (eyes + 0.025 - z) / 0.04))
                   for z in heights]
        outer.append([centre + Vector((0, 0, z)) + d * (r + gap)
                      for z, d, r, gap in zip(heights, directions, radius, padding)])
        inner.append([p - d * 0.002 for p, d in zip(outer[-1], directions)])
    inside = centre + Vector((0, 0, eyes))
    for rings in (outer, inner):
        for row in range(rows - 1):
            for k in range(segments):
                j = (k + 1) % segments
                face = [rings[row][k], rings[row][j], rings[row + 1][j], rings[row + 1][k]]
                if rings is outer:
                    gear.add(face, "cap", inside)
                else:
                    gear.add(list(reversed(face)), "cap", inside + (face[0] - inside) * 2)
        apex = centre + Vector((0, 0, top + (0.0045 if rings is outer else 0.0025)))
        for k in range(segments):
            face = [rings[-1][k], rings[-1][(k + 1) % segments], apex]
            middle = sum(face, Vector()) / len(face)
            gear.add(face, "cap", inside if rings is outer else inside + (middle - inside) * 2)
    for k in range(segments):
        j = (k + 1) % segments
        face = [inner[0][k], inner[0][j], outer[0][j], outer[0][k]]
        gear.add(face, "cap", sum(face, Vector()) / 4 + Vector((0, 0, 0.02)))


def dress(body, rig, regions, material_slot):
    """Long sleeves, full-finger gloves and tights, with a separate zipped gilet shell."""
    mesh = body.data
    names = [g.name for g in body.vertex_groups]

    def weight(vertex, prefixes):
        return sum(g.weight for g in vertex.groups if names[g.group].startswith(prefixes))

    vest_faces = set()
    for polygon in mesh.polygons:
        kind = mesh.materials[polygon.material_index].name
        if kind == "jersey":
            vest_faces.add(polygon.index)
            polygon.material_index = material_slot(body, "sleeve")
        elif kind == "shorts":
            polygon.material_index = material_slot(body, "tights")
        elif kind == "skin":
            verts = [mesh.vertices[v] for v in polygon.vertices]
            region = max(regions, key=lambda r: sum(weight(v, regions[r]) for v in verts))
            choice = {"hands": "gloves", "forearm": "sleeve", "upper_arm": "sleeve",
                      "shin": "tights", "thigh": "tights"}.get(region)
            if choice:
                polygon.material_index = material_slot(body, choice)

    # The gilet copies the torso's weights so it follows the spine and shoulders without
    # clipping through a separately animated jacket. Its closed hem stands off the base layer.
    vest = body.copy()
    vest.data = mesh.copy()
    bpy.context.scene.collection.objects.link(vest)
    shape = bmesh.new()
    shape.from_mesh(vest.data)
    shape.faces.ensure_lookup_table()
    bmesh.ops.delete(shape, geom=[f for f in shape.faces if f.index not in vest_faces],
                     context="FACES")
    shape.normal_update()
    for vertex in shape.verts:
        vertex.co += vertex.normal * 0.006
    shape.to_mesh(vest.data)
    shape.free()
    bpy.context.view_layer.objects.active = vest
    lighter = vest.modifiers.new("fabric detail", "DECIMATE")
    lighter.ratio = 0.25
    bpy.ops.object.modifier_apply(modifier=lighter.name)
    shape = bmesh.new()
    shape.from_mesh(vest.data)
    boundary = [v for v in shape.verts if v.is_boundary]
    for _step in range(8):
        bmesh.ops.smooth_vert(shape, verts=boundary, factor=0.5,
                              use_axis_x=True, use_axis_y=True, use_axis_z=True)
    skin = BVHTree.FromPolygons([v.co for v in mesh.vertices],
                               [list(p.vertices) for p in mesh.polygons])
    for vertex in shape.verts:
        point, normal, _, _ = skin.find_nearest(vertex.co)
        vertex.co = point + normal * 0.008
    for x in (-0.0025, 0.0025):
        bmesh.ops.bisect_plane(shape, geom=list(shape.verts) + list(shape.edges) + list(shape.faces),
                              plane_co=Vector((x, 0, 0)), plane_no=Vector((1, 0, 0)))
    shape.to_mesh(vest.data)
    shape.free()
    vest.data.update()
    gilet = material_slot(vest, "gilet")
    zipper = material_slot(vest, "zip")
    chest = rig.pose.bones["chest"]
    axis = (chest.tail - chest.head).normalized()
    forward = (Vector((0, 1, 0)) - axis * axis.y).normalized()
    for polygon in vest.data.polygons:
        front = polygon.normal.dot(forward) > 0.25
        polygon.material_index = zipper if abs(polygon.center.x) < 0.0026 and front else gilet
    bpy.context.view_layer.objects.active = vest
    hem = vest.modifiers.new("closed hems", "SOLIDIFY")
    hem.thickness = 0.002
    hem.offset = -1.0
    bpy.ops.object.modifier_apply(modifier=hem.name)
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    vest.select_set(True)
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
