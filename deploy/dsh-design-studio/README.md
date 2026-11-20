# DSH 设计工作台

美工从 DSH 侧栏进入 **设计工作台**，选择前端工作区，在 OpenPencil 网页编辑器绘制和保存草稿。

## 使用 AI 设计

点击画布下方的 AI 输入栏，在模型菜单选择当前 DSH 账号有权使用的模型。可以输入页面描述生成可编辑图层，选中图层后描述修改，或在输入栏选择多个 Agent 并行生成多个画板。使用支持图片输入的模型时，上传截图或线框图，并在提示词中写明「参考这张图」。DSH 内置模型使用现有的账号权限、额度和用量记录。画布的 **设置 → 智能体** 也支持 OpenPencil 原有的内置 API 提供商，配置保存在当前浏览器账号分区。

## 绑定前端 Git

在 DSH 登记前端仓库的检出目录，并在设计工作台选中它。新草稿记录该工作区的远端仓库和当前分支。已有草稿可点击 **绑定当前分支**。切换分支后可以重新绑定，旧定稿保留交付时的仓库和分支。

填写目标页面和交互说明，点击 **定稿并开发**。网关核对绑定，将 `.op` 定稿写入对应前端工作区，并通过 DSH 任务看板启动开发会话。同一版定稿会复用原任务。

交付记录中的 **手机预览** 和 **复制预览链接** 指向当时定稿的版本。手机浏览器打开链接并登录 DSH 后，有项目访问权限的成员可以换页、缩放和拖动画布。验证开发后的页面交互时，使用业务项目运行后提供的预览地址。

## 导入 Figma

1. 在 Figma Design 菜单选择 **File → Save local copy**，保存 `.fig` 文件。
2. 在 DSH **设计工作台**选择前端工作区，点击 **导入 Figma**，选择该文件。
3. 在 OpenPencil 画布检查并编辑新草稿。填写目标页面和交互说明，点击 **定稿并开发**启动任务。

导入入口接收本地 `.fig` 文件。浏览器完成转换后，DSH 将可编辑的 `.op` 草稿保存到所选项目名下。转换后的设计稿上限为 16 MiB。大型设计可以拆成多个 `.fig` 文件导入。

## 接入 DSH

补丁以 `dsh-passwords` 的 `8924a8e90999845d75717b5cd60d361d10614d53` 为基线，适用于 DeepSeek Harness `0.2.2-rc.4`。使用 Node.js `22.19` 及以上的 22.x 版本，或 Node.js `24` 及以上版本。网关源码通过 `file:` 依赖读取 Harness 的构建产物，请将已构建的 Harness 检出放在 `dsh-passwords/../../deepseek-harness`。

从同一个 OpenPencil 提交构建 Rust 服务端和浏览器产物。浏览器构建需要 `wasm32-unknown-unknown`、`wasm-bindgen` 和 `wasm-opt`。

```bash
OPENPENCIL_ROOT=/path/to/openpencil
OPENPENCIL_BUNDLE=/path/to/staging/web-bundle
cd "$OPENPENCIL_ROOT"
git submodule update --init --recursive
cargo build --locked --release -p op-host-web-server
bash tools/check-wasm-bundle.sh
mkdir -p "$OPENPENCIL_BUNDLE"
cp -R crates/op-host-web/pkg/. "$OPENPENCIL_BUNDLE/"
cp -R crates/op-host-web/assets/canvaskit "$OPENPENCIL_BUNDLE/canvaskit"

cd /path/to/dsh-passwords
for patch in "$OPENPENCIL_ROOT"/deploy/dsh-design-studio/*.patch; do
  git apply --check "$patch" && git apply "$patch" || exit 1
done
npm ci
DSH_OPENPENCIL_WEB_BUNDLE="$OPENPENCIL_BUNDLE" npm pack
```

构建产物为 `dsh-passwords-2.7.8-dsh.20261119.3.tgz`。包内包含原生网页编辑器、Figma 转换模块，以及固定版本的 OpenPencil 手机查看器。

使用该包更新账号网关和 Harness Web Profile 中的 `dsh-passwords`。在 Web Profile 服务环境中将 `DSH_OPENPENCIL_EDITOR_BIN` 指向同一提交构建的 `op-host-web-server` 绝对路径，启用 `MCP_TENANT_TASK_BOARD=true`，并将 `MCP_MANAGED_WORKSPACE_ROOT` 指向前端工作区所在的数据盘。在 30 号机上，构建、候选包和验证目录使用 `/srv/dsh-data`。

美工账号需要所选前端工作区的访问和写入权限。使用 HTTPS 访问编辑器，手机通过同一个 DSH 网关查看定稿。
