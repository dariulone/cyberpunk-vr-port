-- Written by tools/vr_lobby_assets/make_construct.py: the construct appearance, the resources (path hashes)
-- of the body meshes that carry it -- a component holding one of them wears it -- and the construct's own
-- copies of the meshes that cannot carry it (the component is pointed at the copy while it is worn, its
-- chunk mask set to mask: the chunks the construct shows).
return {
  appearance = "vrport_construct",
  worn = {
    ["17351060424129901860"] = { path = [[base\characters\common\player_base_bodies\player_female_average\arms_hq\a0_000_pwa_base_hq__l.mesh]], own = [[vrport\construct\a0_000_pwa_base_hq__l.mesh]], mask = 0xFFFFFFFFFFFFFFF7ULL },
    ["6127661484476729462"] = { path = [[base\characters\common\player_base_bodies\player_female_average\arms_hq\a0_000_pwa_base_hq__r.mesh]], own = [[vrport\construct\a0_000_pwa_base_hq__r.mesh]] },
    ["3424520411910737910"] = { path = [[base\characters\common\player_base_bodies\player_female_average\fpp\n0_000_pwa_fpp__neck.mesh]] },
    ["4386203188796562796"] = { path = [[base\characters\common\player_base_bodies\player_female_average\l0_000_pwa_base__cs_flat.mesh]] },
    ["10998646327038800579"] = { path = [[base\characters\common\player_base_bodies\player_female_average\t0_000_pwa_base__full.morphtarget]] },
    ["6123990083473767804"] = { path = [[base\characters\common\player_base_bodies\player_man_average\arms_hq\a0_000_pma_base_hq__l.mesh]], own = [[vrport\construct\a0_000_pma_base_hq__l.mesh]], mask = 0xFFFFFFFFFFFFFFF7ULL },
    ["3536741777257648318"] = { path = [[base\characters\common\player_base_bodies\player_man_average\arms_hq\a0_000_pma_base_hq__r.mesh]], own = [[vrport\construct\a0_000_pma_base_hq__r.mesh]] },
    ["10166971776422861834"] = { path = [[base\characters\common\player_base_bodies\player_man_average\fpp\n0_000_pma_fpp__neck.mesh]] },
    ["3781668235714414901"] = { path = [[base\characters\common\player_base_bodies\player_man_average\t0_000_pma_base__full.mesh]] },
    ["17923313006320215944"] = { path = [[base\characters\garment\player_equipment\feet\s1_010_shoe__elegant\s1_010_pma_shoe__elegant.mesh]] },
  },
  own = {
    ["4348962040142790935"] = true,
    ["13207257604266595469"] = true,
    ["14785570617216477197"] = true,
    ["5927275053092672663"] = true,
  },
}
