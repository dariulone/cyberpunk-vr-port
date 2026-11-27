"""pistol_variants.py OUT: every variant of the slide pistols given and drawn in the running game (devgame.py start), and
which reload config took it -- the name log/reload_reach.json carries once the gun's slide is offered to the free hand
(nothing written: the variant is not recognised). One line per variant in OUT/variants.txt."""
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import devgame as D  # noqa: E402
import slide_grab as S  # noqa: E402

FAMILIES = {
    'arasaka_kenshin': 'Kenshin_Default Kenshin_Frank Kenshin_Frank_Legendary Kenshin_MQ041 Kenshin_Military Kenshin_Neon Kenshin_Pimp '
                       'Kenshin_Royce Kenshin_Royce_Epic Kenshin_Royce_Legendary Kenshin_Takemura Kenshin_Default_Left_Hand Kenshin_Default_Retrofix',
    'constitutional_liberty': 'Liberty_Default Liberty_Dex Liberty_Dex_Epic Liberty_Dex_Legendary Liberty_Military Liberty_Neon Liberty_Padre '
                              'Liberty_Padre_Legendary Liberty_Pimp Liberty_Rogue Liberty_Rogue_Stash Liberty_Yorinobu Liberty_Yorinobu_Epic '
                              'Liberty_Yorinobu_Legendary Liberty_Default_Left_Hand Liberty_Default_Retrofix',
    'constitutional_unity': 'Unity_Default Unity_Military Unity_Neon Unity_Pimp V_Unity Unity_Default_Left_Hand Unity_Default_Retrofix',
    'malorian_silverhand': 'Silverhand_3516',
    'militech_lexington': 'Base_Lexington Lexington_Default Lexington_Military Lexington_Neon Lexington_Pimp Lexington_Shooting_Competition '
                          'Lexington_Toygun Lexington_Training Lexington_Wilson Lexington_Wilson_Epic Lexington_Wilson_Legendary '
                          'Lexington_Wilson_Rare Q001_Lexington Lexington_Default_Left_Hand Lexington_Default_Retrofix',
    'tsunami_nue': 'Nue_Arasaka_2020 Nue_Arasaka_2077 Nue_Boris Nue_Default Nue_Jackie Nue_Jackie_Epic Nue_Jackie_Legendary Nue_Maiko '
                   'Nue_Maiko_Epic Nue_Maiko_Legendary Nue_Military Nue_Neon Nue_Pimp Nue_Default_Left_Hand Nue_Default_Retrofix',
}


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    D.cmd('reach 1')
    rows, bad = [], []
    t_all = time.time()
    for want, names in FAMILIES.items():
        for name in names.split():
            rec = f'Items.Preset_{name}'
            before = S.reach().get('t', -1.0)
            r = D.cmd(f'weapon {rec}', wait='active weapon', timeout=20)
            drawn = next((ln.split('active weapon ')[1].split(' (')[0] for ln in r.splitlines() if 'active weapon' in ln), '-')
            acc = S.fresh({'t': before}, until=lambda x: x.get('weapon') == want, most=2.5)
            got = acc.get('weapon') if acc.get('t', -1.0) > before else None
            ok = drawn == rec and got == want
            rows.append(f'{"ok " if ok else "BAD"} {rec}: drawn {drawn == rec}, config {got}')
            if not ok:
                bad.append(rows[-1])
            print(rows[-1], flush=True)
    rows.append(f'{len(rows)} variants in {time.time() - t_all:.0f} s, {len(bad)} not recognised')
    print(rows[-1], flush=True)
    (out / 'variants.txt').write_text('\n'.join(rows), encoding='utf-8')


if __name__ == '__main__':
    main()
