# DSH 设计工作台

自托管的 DSH 账号网关可集成 OpenPencil 画布。美工从侧栏进入 **设计工作台**，选择业务项目，在网页绘制并保存草稿。点击 **定稿并开发** 后，网关把这版 `.op` 写到所选项目，并通过 DSH 任务看板启动开发会话。对同一版设计重复交付时，任务看板复用原任务。

交付记录中的 **手机预览** 和 **复制预览链接** 指向当时定稿的版本。手机浏览器打开链接并登录 DSH 后，有项目访问权限的成员可以换页、缩放和拖动画布。验证开发后的页面交互时，使用业务项目运行后提供的预览地址。

## 接入 DSH

补丁以 `dsh-passwords` 的 `8924a8e90999845d75717b5cd60d361d10614d53` 为基线，适用于 DeepSeek Harness `0.2.2-rc.4` 环境。使用 Node.js `22.19` 及以上的 22.x 版本，或 Node.js `24` 及以上版本。在 DSH 插件的干净工作区应用补丁。

```bash
OPENPENCIL_ROOT=/path/to/openpencil
cd /path/to/dsh-passwords
git apply --check "$OPENPENCIL_ROOT"/deploy/dsh-design-studio/*.patch
git apply "$OPENPENCIL_ROOT"/deploy/dsh-design-studio/*.patch
npm ci
npm pack
```

打包会校验并嵌入固定版本的 OpenPencil `0.1.0-rc.15` 浏览器编辑器和查看器。构建产物为 `dsh-passwords-2.7.8-dsh.20261119.1.tgz`。

使用该包更新账号网关和 Harness Web Profile 中的 `dsh-passwords`。启用 `MCP_TENANT_TASK_BOARD=true`，让 `MCP_MANAGED_WORKSPACE_ROOT` 与项目工作区位于同一数据盘。美工账号需要目标项目的访问和写入权限。编辑器需要 HTTPS，手机需要能够访问同一个 DSH 网关。
