# Spring (#350, R155)

Spring opens five-petal meadow flowers at the existing flower positions: the shader
widens the heads around their UV2 centre while leaving the stems attached. The procedural
mesh uses 10 triangles per head (30 per clump); grass density, placement and exclusions are unchanged.
Summer retains smaller flower heads and the original flower palette.

Stable tree-position seeds select about a third of broadleaf crowns for pink and white
blossom among the green. Flowering crowns sample a dedicated transparent five-petal blossom
atlas, with twig clusters and warm stamen centres, in place of the leaf silhouettes. The
1024 px lossless PNG is reproducibly drawn by `art/vegetation/foliage.py --blossoms-only`,
VRAM compressed with mipmaps on import, and tinted pink or white by the seasonal palette.
Summer restores the original leaf atlas. This is a seasonal visual treatment of the existing broadleaf
models, not a claim of mapped cherry species. Needles stay green; Rust's climate ceiling
keeps tropical lowlands evergreen. All authored LODs share the same materials, and the
distant canopy has light blossom patches without replacing its relief.

Review Spring and Summer at identical Gurten cameras, a meadow flower close-up, and Low's
distant woods. Shader and procedural presentation-mesh changes need no generator or
stored-format increment, no external assets and no dependencies.

Reviewed [open flowers](images/seasons/spring-flowers.jpg), a
[blossoming crown](images/seasons/spring-blossom.jpg), and the same route on
[Medium](images/seasons/spring-hairpin.jpg) and [Low](images/seasons/spring-low.jpg).
