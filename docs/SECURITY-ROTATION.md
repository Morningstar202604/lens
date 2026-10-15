# 安全操作记录：CA 私钥历史清除 与 访问令牌轮换

## 1. Git 历史中的演示 CA 私钥（demo_data/certs/ca.key）—— 已完成

**状态：已于 2026-10-15 清除完毕。**

该文件此前已从索引移除（`.gitignore` 覆盖），但仍存在于多个历史提交中。仓库是公开的，
保留私钥属于卫生问题，因此做了历史重写。

执行方式：`git filter-repo --force --invert-paths --paths-from-file`（移除
`demo_data/certs/ca.key` 与 `ca.crt`），随后 `git reflog expire --expire=now --all` +
`git gc --prune=now` 清理本地残留对象，四个远端（origin/gitee/x33834/ms2026）均已
force push 到重写后的历史。

验证证据（**在干净克隆的远端副本上实测**，非本地自查）：

```bash
git clone https://gitcode.com/badhope/lens.git /tmp/verify
cd /tmp/verify
git log --all --oneline -- "demo_data/certs/*" "*.key"     # 无输出
for c in $(git rev-list --all); do
  git grep -l -E "BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----$" $c -- | grep -v node_modules
done                                                        # 无输出
```

说明：`frontend/node_modules/esbuild/bin/esbuild` 二进制内含字面量字符串
"PRIVATE KEY"，会被朴素 grep 命中——那是 esbuild 自身的代码常量，不是密钥材料。

该 CA 从未装入任何系统信任库（本地无 `certs/` 目录），无需额外撤销。

### 未完成项：本地备份包

清除前生成的完整历史快照位于 `C:/tmp/lens-backup/lens-all.bundle`（19MB），
**其中仍包含被清除的 CA 私钥**（它是过滤前的快照，仅作回滚用）。
若确认不再需要回滚，请删除：

```bash
rm C:/tmp/lens-backup/lens-all.bundle
```

### 历史步骤留档（供其他仓库复用）

```bash
# 1) 全量备份（回滚用）
git bundle create /tmp/lens-all.bundle --all      # 或 git clone --mirror <url>

# 2) 用 git-filter-repo 删除路径（需先 pip install git-filter-repo）
printf 'demo_data/certs/ca.key\ndemo_data/certs/ca.crt\n' > /tmp/paths.txt
git filter-repo --force --invert-paths --paths-from-file /tmp/paths.txt

# 3) 清理本地残留对象，避免旧历史仍可达
git reflog expire --expire=now --all && git gc --prune=now

# 4) filter-repo 会移除 remote 配置，恢复后再强制推送全部远端
git remote add origin https://gitcode.com/badhope/lens.git
git push --force --all && git push --force --tags

# 5) 用干净克隆验证（在别处 clone 远端，再跑上面的 grep 检查）
```

## 2. 访问令牌轮换 —— 仍需在提供商侧执行

**当前状态：本机凭据库中的 GitCode 令牌与曾在对话中明文出现的是同一枚**
（以 sha256 前缀比对确认，未回显明文）。轮换必须在提供商控制台完成，脚本无法代做：

1. GitCode → 设置 → 访问令牌 → 删除旧令牌 → 新建
2. 更新本机凭据库后同步全部出口：
   `python ~/.zcode/secrets/accounts_cli.py apply`（= sync + store-git + win-env）
3. 验证：`python ~/.zcode/secrets/accounts_cli.py check`（只输出状态与 login，不回显密钥）

轮换前该令牌仍可用于访问该账号的仓库，属已知暴露面。

## 3. 远端 URL 中的内嵌凭据 —— 已完成

远端 URL 原先形如 `https://oauth2:<token>@gitee.com/...` 与 `https://<user>:<pat>@github.com/...`，
即令牌明文写在 `.git/config` 里（任何能读该文件的进程或误贴的 `git remote -v` 输出都会泄露）。

已改为不含凭据的干净 URL，认证交给 git credential helper（Windows 凭据管理器，加密存储）：

```bash
git remote set-url origin https://gitcode.com/badhope/lens.git
git remote set-url gitee  https://gitee.com/badhope/lens.git
git remote set-url x33834 https://github.com/X33834/lens.git
git remote set-url ms2026  https://github.com/Morningstar202604/lens.git
python ~/.zcode/secrets/accounts_cli.py store-git   # 注入全部凭据
git ls-remote --heads origin                        # 验证干净 URL 可认证
```
