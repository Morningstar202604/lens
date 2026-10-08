#!/usr/bin/env python3
"""Seed the Lens database with a rich Mushoku Tensei (无职转生) knowledge graph.

直接写入 nodes / edges 表 — 与 agent 抽取的图谱共存。
节点种类:
  character   — 角色
  location    — 地点
  family      — 家族
  event       — 大事件
  skill       — 技能/魔法
  artifact    — 物品/装备
"""

import os
import sqlite3
import sys

DB_PATH = "./demo_data/lens.db"

NODES = [
    # ──────────────── 主角与核心角色 ────────────────
    ("char:rudeus",     "鲁迪乌斯·格雷拉特",  "character", 10.0),
    ("char:roxy",       "罗琪希·米格路德尼亚", "character",  9.0),
    ("char:eris",       "艾利斯·伯雷斯博",    "character",  9.0),
    ("char:sylphiette", "菲兹",              "character",  8.5),
    ("char:paul",       "保罗·格雷拉特",      "character",  8.0),
    ("char:zenith",     "塞妮丝·格雷拉特",    "character",  8.0),
    ("char:lilia",      "莉希安·格雷拉特",    "character",  7.0),
    ("char:apex",       "阿托斯尔·格雷拉特",  "character",  7.0),
    ("char:norn",       "诺伦·格雷拉特",      "character",  7.5),
    ("char:aisha",      "爱莎·格雷拉特",      "character",  7.0),

    # ──────────────── 第二篇章：魔法大陆 ────────────────
    ("char:ruijerd",    "鲁杰鲁德·斯佩路德", "character",  8.0),
    ("char:elinalise",  "伊娜娜·龙",        "character",  7.0),
    ("char:talhand",    "塔尔汉德·龙",      "character",  6.5),
    ("char:joris",      "乔里斯·斯佩路德",  "character",  7.5),

    # ──────────────── 第三篇章：学园 ────────────────
    ("char:linia",      "莉娜里亚·龙",      "character",  6.5),
    ("char:perugius",   "佩鲁基乌斯·龙",    "character",  6.0),
    ("char:badigadi",   "巴迪·冈迪",        "character",  6.0),
    ("char:kishirika",  "基希莉卡·龙",      "character",  6.5),
    ("char:areida",     "阿列达·龙",        "character",  5.5),
    ("char:samazuka",   "萨玛·斯卡兹卡",    "character",  5.5),
    ("char:arumanfi",   "阿尔马菲·龙",      "character",  5.5),

    # ──────────────── 拉普拉斯战役阵营 ────────────────
    ("char:laplace",    "拉普拉斯",          "skill",      9.5),
    ("char:ort foutis","奥尔提·佛提斯",    "character",  6.0),
    ("char:hitogami",   "人神 (希托加米)",   "character",  9.0),

    # ──────────────── 地点 ────────────────
    ("loc:buena",       "布耶那村",        "location",    9.0),
    ("loc:niik",        "尼克镇",          "location",    6.5),
    ("loc:migurdia",    "米格路德尼亚王都", "location",    7.5),
    ("loc:bold",        "伯拉斯王国",      "location",    7.0),
    ("loc:magic_city",  "魔法都市·玛希尔", "location",    8.0),
    ("loc:boreas",      "伯雷亚斯领地",    "location",    6.5),
    ("loc:fittoa",      "菲托亚领地·迁徙地", "location",  8.0),
    ("loc:holy_kingdom","圣王国·帕西凡",  "location",    7.0),
    ("loc:rano",        "拉诺亚魔法大学",  "location",    9.0),
    ("loc:demon_world", "魔大陆",          "location",    7.5),
    ("loc:hea",         "龙神殿",           "location",    6.0),

    # ──────────────── 家族 ────────────────
    ("family:greyrat",  "格雷拉特家族",    "family",     10.0),
    ("family:boreas",   "伯雷亚斯家族(龙)", "family",      8.5),
    ("family:migurd","米格路德尼亚王族",  "family",      8.0),
    ("family:spears","斯佩路德族",        "family",      7.0),
    ("family:vollkiser","沃尔卡伊瑟家",   "family",      6.0),

    # ──────────────── 大事件 ────────────────
    ("event:mana_calm", "大灾害·魔力紊乱", "event",      8.0),
    ("event:exile",     "菲托亚领地大转移", "event",    8.5),
    ("event:lost_child","拉普拉斯之战·失子", "event",    8.0),
    ("event:academy",   "魔法大学入学",    "event",      7.0),
    ("event:birth",     "鲁迪儿子出生",    "event",      7.5),
    ("event:reunion",   "家庭重逢",        "event",      9.0),

    # ──────────────── 技能/魔法 ────────────────
    ("skill:voice",     "咏唱魔术无咏唱",  "skill",      9.0),
    ("skill:stone_canon","石弹 (Stone Cannon)", "skill", 8.0),
    ("skill:gravity",   "重力魔法",        "skill",      7.5),
    ("skill:sylphiette_spirit","菲兹精灵术", "skill",  7.5),
    ("skill:roxi_treaty","罗琪希水之加护", "skill",     7.0),
    ("skill:talgross","塔纳德·龙神拳",     "skill",      6.5),
    ("skill:demon_eye", "魔王之眼",        "skill",      8.5),

    # ──────────────── 物品/神器 ────────────────
    ("artif:lindwurm","龙神·林德龙特甲",  "artifact",    6.5),
    ("artif:imperial","魔导铠·皇帝",      "artifact",    7.0),
    ("artif:ocean_staff","水魔法之杖",    "artifact",    6.0),
    ("artif:quagmire_staff","泥沼之杖",  "artifact",    6.0),
    ("artif:teleport","转移魔法阵",      "artifact",    6.5),

    # ──────────────── 思想/主题 ────────────────
    ("theme:redemeption","前世今生·救赎",  "theme",      10.0),
    ("theme:family",    "家族与羁绊",      "theme",      9.5),
    ("theme:growup",    "成长与责任",      "theme",      9.0),
    ("theme:otherworld","异世界转移",      "theme",      8.0),
    ("theme:isolation", "孤立与孤独",      "theme",      8.0),
]

EDGES = [
    # 家族关系
    ("char:rudeus",    "family:greyrat",   "belongsTo",   10),
    ("char:paul",      "family:greyrat",   "belongsTo",   10),
    ("char:zenith",    "family:greyrat",   "belongsTo",   10),
    ("char:norn",      "family:greyrat",   "belongsTo",   10),
    ("char:aisha",     "family:greyrat",   "belongsTo",   10),
    ("char:lilia",     "family:greyrat",   "belongsTo",   10),
    ("char:apex",      "family:greyrat",   "belongsTo",   10),
    ("char:paul",      "char:zenith",      "marriedTo",   10),
    ("char:norn",      "char:rudeus",      "sisterOf",     9),
    ("char:aisha",     "char:rudeus",      "sisterOf",     9),
    ("char:apex",      "char:rudeus",      "guardianOf",   7),

    # 师徒关系
    ("char:rudeus",    "char:roxy",        "studentOf",   10),
    ("char:sylphiette","char:roxy",        "studentOf",    8),
    ("char:rudeus",    "char:elinalise",   "studiedBy",    6),

    # 夫妻关系
    ("char:rudeus",    "char:eris",        "marriedTo",   10),
    ("char:rudeus",    "char:sylphiette",  "marriedTo",   10),
    ("char:eris",      "char:sylphiette",  "fellowWifeOf",8),

    # 地理位置
    ("char:rudeus",    "loc:buena",        "bornIn",      9),
    ("char:rudeus",    "loc:migurdia",     "mentoredIn",  8),
    ("char:roxy",      "loc:migurdia",     "from",        8),
    ("char:rudeus",    "loc:boreas",       "guardianOf",  7),
    ("char:rudeus",    "loc:fittoa",       "wandered",   10),
    ("char:paul",      "loc:fittoa",       "wandered",    9),
    ("char:zenith",    "loc:fittoa",       "lostIn",     10),
    ("char:rudeus",    "loc:rano",         "studiedAt",  10),
    ("char:sylphiette","loc:rano",         "studiedAt",   9),
    ("family:greyrat", "loc:migurdia",     "livesIn",     8),

    # 家族 — 地点
    ("family:boreas",  "loc:magic_city",   "rules",       8),
    ("family:migurd",  "loc:migurdia",     "rules",       9),
    ("family:boreas",  "loc:boreas",       "rules",       8),

    # 大事件关联
    ("loc:fittoa",     "event:exile",      "locationOf", 10),
    ("event:exile",    "char:paul",        "involves",    9),
    ("event:exile",    "char:zenith",      "involves",    9),
    ("event:exile",    "char:rudeus",      "survivedBy",  9),
    ("event:exile",    "char:roxy",        "mentorOf",    9),
    ("event:reunion",  "char:rudeus",      "foundFamily", 10),
    ("char:rudeus",    "event:academy",    "attends",     8),
    ("char:sylphiette","event:academy",    "attends",     7),
    ("event:mana_calm", "loc:buena",       "aftermathOf", 8),
    ("event:mana_calm", "event:exile",     "causedBy",   9),
    ("loc:boreas",     "event:birth",      "locationOf",  7),

    # 技能关联
    ("char:rudeus",    "skill:voice",      "mastered",   10),
    ("char:rudeus",    "skill:stone_canon", "signatureOf", 9),
    ("char:rudeus",    "skill:gravity",    "uses",        8),
    ("char:rudeus",    "skill:demon_eye",  "givenBy",     7),
    ("skill:demon_eye","char:perugius",    "grantedBy",   8),
    ("char:sylphiette","skill:sylphiette_spirit","mastered", 9),
    ("char:roxy",      "skill:roxi_treaty","mastered",   8),
    ("char:talhand",   "skill:talgross",   "mastered",    7),

    # 装备关联
    ("char:rudeus",    "artif:quagmire_staff","wields",   8),
    ("char:rudeus",    "artif:teleport",   "uses",        6),
    ("artif:lindwurm", "char:ort foutis",  "wornBy",      7),
    ("artif:imperial", "char:areida",      "wornBy",      7),

    # 主题关联
    ("char:rudeus",    "theme:redemeption","embodies",   10),
    ("char:paul",      "char:rudeus",      "redeemedBy",  8),
    ("theme:family",   "char:rudeus",      "centralTo",  9),
    ("theme:family",   "char:zenith",      "drivenBy",    8),
    ("theme:growup",   "char:rudeus",      "definesArc",   10),
    ("theme:isolation","char:rudeus",      "strugglesWith", 8),
    ("theme:otherworld","char:rudeus",     "revolvesAround", 9),

    # 敌对/冲突
    ("char:rudeus",    "char:hitogami",    "manipulatedBy", 8),
    ("char:hitogami",  "theme:isolation",  "causes",       7),
    ("char:rudeus",    "char:laplace",     "foughtAgainst", 9),
    ("char:laplace",   "event:lost_child", "causedBy",    9),
    ("char:ruijerd",   "char:rudeus",      "befriends",    7),
    ("family:spears",  "loc:demon_world",  "nativeTo",     8),
    ("char:ruijerd",   "family:spears",    "memberOf",     8),

    # 神器/传承
    ("artif:teleport", "event:exile",      "originOf",     9),
    ("loc:hea",        "char:perugius",    "templeOf",     8),
]


def main():
    db_path = sys.argv[1] if len(sys.argv) > 1 else DB_PATH
    os.makedirs(os.path.dirname(db_path) or ".", exist_ok=True)

    conn = sqlite3.connect(db_path)
    # 建表（若不存在）
    conn.executescript("""
        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY, label TEXT NOT NULL, kind TEXT NOT NULL,
            weight REAL DEFAULT 1.0, first_seen DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL, target TEXT NOT NULL,
            relation TEXT NOT NULL, weight REAL DEFAULT 1.0,
            UNIQUE(source, target, relation)
        );
    """)

    # 清空旧数据，重新写入（幂等）
    conn.execute("DELETE FROM nodes")
    conn.execute("DELETE FROM edges")

    conn.executemany(
        "INSERT INTO nodes (id, label, kind, weight) VALUES (?, ?, ?, ?)",
        NODES,
    )
    conn.executemany(
        "INSERT OR IGNORE INTO edges (source, target, relation, weight) VALUES (?, ?, ?, ?)",
        EDGES,
    )

    conn.commit()

    node_count = conn.execute("SELECT COUNT(*) FROM nodes").fetchone()[0]
    edge_count = conn.execute("SELECT COUNT(*) FROM edges").fetchone()[0]
    print(f"✅ 知识图谱已生成: {node_count} 个节点, {edge_count} 条边")
    print(f"   种类: character / location / family / event / skill / artifact / theme")
    conn.close()


if __name__ == "__main__":
    main()
