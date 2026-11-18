use super::*;

fn wall(style: u8, offset: f64) -> MeshData {
    let mut mesh = MeshData::default();
    quad_uv(
        &mut mesh,
        [
            [offset, -1.0, 0.0],
            [offset + 12.8, -1.0, 0.0],
            [offset + 12.8, 12.4, 0.0],
            [offset, 12.4, 0.0],
        ],
        [[0.0, -1.0], [12.8, -1.0], [12.8, 12.4], [0.0, 12.4]],
        [0.0, 0.0, 1.0],
        [0.8, 0.7, 0.6, f32::from(style) / 11.0],
    );
    mesh
}

#[test]
fn close_details_retain_the_saved_shell_and_match_its_window_grid() {
    let shell = wall(0, 0.0);
    let saved = shell.clone();
    let details = building_details(&shell);
    assert_eq!(shell, saved, "quality must not change kept world data");
    let panes: Vec<_> = details
        .iter()
        .flat_map(|mesh| mesh.vertices.iter().zip(&mesh.colors))
        .filter(|(_, color)| (color[3] - 0.5).abs() < 1e-5)
        .collect();
    assert_eq!(
        panes.len(),
        4 * 4 * 4,
        "four columns and four storeys of glazing"
    );
    for (point, _) in panes {
        let u = point[0] % 3.2;
        assert!((0.959..=2.241).contains(&u));
        assert!(
            (point[2] - 0.014).abs() < 1e-5,
            "glass sits behind the raised frame"
        );
    }
}

#[test]
fn streamed_details_follow_the_view_without_changing_saved_surfaces() {
    let mut shell = wall(0, 0.0);
    shell.append(wall(0, 1000.0));
    let saved = shell.clone();
    let near = nearby_building_details(&shell, [0.0, 0.0], 100.0);
    let far = nearby_building_details(&shell, [1000.0, 0.0], 100.0);
    assert!(!near.is_empty() && !far.is_empty());
    assert!(near.iter().flat_map(|m| &m.vertices).all(|p| p[0] < 100.0));
    assert!(far.iter().flat_map(|m| &m.vertices).all(|p| p[0] > 900.0));
    assert_eq!(shell, saved);
    assert_eq!(nearby_building_details(&shell, [f64::NAN, 0.0], 100.0), []);
}

#[test]
fn blanks_roofs_and_narrow_panels_do_not_receive_invented_windows() {
    for style in [1, 3, 4, 5, 7, 8, 9] {
        assert_eq!(building_details(&wall(style, 0.0)), []);
    }
    let mut narrow = wall(0, 0.0);
    for uv in &mut narrow.uvs {
        uv[0] *= 0.05;
    }
    let detail = building_details(&narrow);
    assert!(
        detail
            .iter()
            .all(|mesh| mesh.colors.iter().all(|c| (c[3] - 0.5).abs() > 1e-5))
    );
}

#[test]
fn panels_are_batched_locally_and_rotated_facades_keep_outward_faces() {
    let mut shell = wall(2, -170.0);
    shell.append(wall(0, 170.0));
    for point in &mut shell.vertices {
        *point = [-point[2], point[1], point[0]];
    }
    for normal in &mut shell.normals {
        *normal = [-normal[2], normal[1], normal[0]];
    }
    let details = building_details(&shell);
    assert!(
        details.len() >= 2,
        "distant buildings cannot share one visibility bound"
    );
    for mesh in details {
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.iter().all(|f| f.is_finite()))
        );
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let a = mesh.vertices[triangle[0] as usize].map(f64::from);
            let b = mesh.vertices[triangle[1] as usize].map(f64::from);
            let c = mesh.vertices[triangle[2] as usize].map(f64::from);
            let ab = sub(b, a);
            let ac = sub(c, a);
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let normal = mesh.normals[triangle[0] as usize].map(f64::from);
            assert!(dot(cross, normal) < 0.0, "Godot faces wind clockwise");
        }
    }
}

#[test]
fn malformed_or_triangle_only_inputs_are_ignored_without_panicking() {
    let mut shell = wall(0, 0.0);
    shell.indices[1] = u32::MAX;
    assert_eq!(building_details(&shell), []);
    shell.indices.truncate(3);
    assert_eq!(building_details(&shell), []);
    shell.normals.clear();
    assert_eq!(building_details(&shell), []);
}

#[test]
fn shop_fronts_cover_the_house_windows_behind_them() {
    let mut shell = wall(0, 0.0);
    let mut shop = wall(10, 0.0);
    for (point, uv) in shop.vertices.iter_mut().zip(&mut shop.uvs) {
        if point[1] > 0.0 {
            point[1] = 3.0;
            uv[1] = 3.0;
        }
        point[2] = 0.08;
    }
    shell.append(shop);
    let details = building_details(&shell);
    let mut panes = 0;
    for mesh in &details {
        for ((point, color), uv) in mesh.vertices.iter().zip(&mesh.colors).zip(&mesh.uvs) {
            if (color[3] - 0.5).abs() < 1e-5 {
                let shop = point[1] < 3.0;
                assert_eq!(
                    uv[1] >= 1.0,
                    shop,
                    "shop glass shows a sales floor, house glass a home"
                );
                if shop {
                    assert!(
                        point[2] > 0.08,
                        "only shop glazing appears on the ground floor"
                    );
                    panes += 1;
                }
            }
        }
    }
    assert_eq!(panes, 16);
}

#[test]
#[ignore = "exports controlled geometry for Godot visual review"]
fn building_detail_render_fixture() {
    let mut shell = MeshData::default();
    for side in 0..4 {
        let mut facade = wall(0, -6.4);
        for point in &mut facade.vertices {
            let x = point[0];
            *point = match side {
                0 => [x, point[1], 6.4],
                1 => [6.4, point[1], -x],
                2 => [-x, point[1], -6.4],
                _ => [-6.4, point[1], x],
            };
        }
        for normal in &mut facade.normals {
            *normal = match side {
                0 => [0.0, 0.0, 1.0],
                1 => [1.0, 0.0, 0.0],
                2 => [0.0, 0.0, -1.0],
                _ => [-1.0, 0.0, 0.0],
            };
        }
        for paint in &mut facade.colors {
            *paint = palette::pick("buildings.walls", 0, 0.0);
        }
        shell.append(facade);
    }
    // A shop on the ground floor of the side the review camera faces.
    let mut shop = wall(10, -6.4);
    for (point, uv) in shop.vertices.iter_mut().zip(&mut shop.uvs) {
        if point[1] > 0.0 {
            point[1] = 3.0;
            uv[1] = 3.0;
        }
        point[2] = 6.48;
    }
    for paint in &mut shop.colors {
        paint[..3].copy_from_slice(&palette::pick("buildings.walls", 2, 0.0)[..3]);
    }
    shell.append(shop);
    quad_uv(
        &mut shell,
        [
            [-6.4, 12.4, -6.4],
            [6.4, 12.4, -6.4],
            [6.4, 12.4, 6.4],
            [-6.4, 12.4, 6.4],
        ],
        [[0.0, 0.0], [12.8, 0.0], [12.8, 12.8], [0.0, 12.8]],
        [0.0, 1.0, 0.0],
        palette::srgb("buildings.stone", 9.0 / 11.0),
    );
    let encode = |mesh: &MeshData| {
        serde_json::json!({
            "vertices": mesh.vertices, "normals": mesh.normals, "uvs": mesh.uvs,
            "colors": mesh.colors, "indices": mesh.indices,
        })
    };
    let details: Vec<_> = building_details(&shell).iter().map(&encode).collect();
    let fixture = serde_json::json!({"shell": encode(&shell), "details": details});
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../screenshots/building-integration");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("fixture.json"),
        serde_json::to_vec(&fixture).unwrap(),
    )
    .unwrap();
}
