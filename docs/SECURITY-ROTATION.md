# 安全操作待办：CA 私钥历史清除 与 访问令牌轮换

两个一次性安全动作。第一项是破坏性历史重写，需你明确确认并由你执行（或授权）后再做；
第二项在任何时间做都无副作用。

## 1. Git 历史中的演示 CA 私钥（demo_data/certs/ca.key）

现状：该文件已从索引移除（工作区保留、`.gitignore` 覆盖），但它仍存在于历史提交中。
只要没有人把这个 CA 装入系统信任库，它本身无害；但仓库是公开的，保留它属于卫生问题。

清除步骤（会改写历史 → 必须 force push，执行前确保没有他人正在基于此仓库工作）：

```bash
# 1) 全量镜像备份（回滚用）
git clone --mirror https://gitcode.com/badhope/lens.git lens-backup.git

# 2) 用 git-filter-repo 删除该路径（pip install git-filter-repo）
git filter-repo --path demo_data/certs --invert-paths

# 3) 强制推送（需要仓库访问令牌；GitCode 走普通 https 凭据）
git push --force --all
git push --force --tags

# 4) 确认历史中不再出现
git log --all --oneline -- demo_data/certs   # 应无输出
```

若曾把该 CA 装入过信任库（本机/CI），一并删除并重新运行 `lens proxy` 生成新 CA。

## 2. 访问令牌轮换

- 曾在对话/终端明文出现过的 GitCode 访问令牌：到 GitCode → 设置 → 访问令牌，删除旧令牌并新建。
- 本机凭据库维护（变更后同步全部出口）：
  `python ~/.zcode/secrets/accounts_cli.py sync && python ~/.zcode/secrets/accounts_cli.py store-git`
- 验证：`python ~/.zcode/secrets/accounts_cli.py check`（只输出状态与 login，不回显密钥）。
