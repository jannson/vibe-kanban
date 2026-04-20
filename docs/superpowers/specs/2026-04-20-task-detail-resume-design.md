# Task Detail Resume Design

**Goal**

为 `Done/Cancelled` 任务提供一种安全、低误操作的“继续任务”机制：任务业务状态保持不变，只有当前页面显式解锁后，才允许进行会话输入、分支检查、diff 查看与编辑器打开等危险动作。

## 背景

当前问题本质上不是单一接口 bug，而是业务概念混杂：

- `TaskStatus` 同时被当作业务生命周期状态和页面访问控制状态使用
- “查看历史任务详情” 与 “恢复到可继续操作态” 没有被清晰区分
- 结果是 `Done` 任务在某些前端挂载链路下会提前触发 workspace prepare / branch switch

此前尝试将 `Done -> InProgress` 作为“继续任务”的实现方式，虽然能让后续接口继续工作，但会污染任务生命周期语义，导致看板状态失真。

## 设计结论

### 1. 保持 `TaskStatus` 的纯业务语义

`TaskStatus` 只表示任务业务生命周期，不因“继续任务”而改变：

- `todo`
- `in_progress`
- `in_review`
- `done`
- `cancelled`

**关键约束：**

- 点击“继续任务”时，`Done` 仍然保持 `Done`
- 点击“继续任务”时，`Cancelled` 仍然保持 `Cancelled`
- 不允许为了放开页面操作而把任务改回 `InProgress`

### 2. 新增独立概念：Attempt Access State

对任务详情页引入独立的访问态，而不是复用 `TaskStatus`：

- `locked`
- `resuming`
- `resumed`

其语义为：

- `locked`：已完成任务默认只读，不允许危险操作
- `resuming`：用户刚点击“继续任务”，正在申请临时解锁
- `resumed`：当前页面已经显式解锁，允许危险操作

### 3. `resumed` 仅在当前页面有效

这是本次设计中最重要的约束之一：

- `resumed` **不持久化到任务状态**
- `resumed` **不持久化到数据库长期字段**
- `resumed` **不跨页面刷新保存**
- `resumed` **不跨标签页共享**

即：

- 在当前页面点击“继续任务”后，可以操作
- 刷新页面后，恢复为 `locked`
- 离开详情页再回来，恢复为 `locked`
- 切换到其他任务后，当前任务的临时解锁状态丢失

这样可以最大限度降低误操作概率。

## 推荐技术模型

### 方案选择

推荐方案是：

**`Done` 保持不变 + 当前页面短期 `resume_key`**

即：

1. 用户点击“继续任务”
2. 后端签发一个只在当前页面生命周期内使用的短期凭证 `resume_key`
3. 前端仅在内存中保存它
4. 后续危险接口必须显式带上该 `resume_key`
5. 页面刷新后，`resume_key` 丢失，页面回到 `locked`

### 为什么不用纯前端布尔值

如果只靠前端 `isResumeUnlocked` 这类内存布尔值：

- 后端不知道本次请求是不是显式继续后的
- 只能靠前端自己“乖乖别调危险接口”
- 一旦某个组件误挂载、旧 query 轮询、URL 视图恢复、缓存残留，都可能再次触发 workspace prepare

因此，**纯前端布尔值不够安全**。

### 为什么不用改任务状态

如果点击“继续任务”就改成 `InProgress`：

- 看板语义被污染
- 已合并任务会重新显示成进行中
- 用户难以区分“业务上又重新开始开发”与“只是临时查看/继续补充”
- 兼容性差，历史统计与状态筛选容易出问题

因此，**不能用修改 `TaskStatus` 来表达详情页访问态**。

## 详细行为定义

### 行为 1：打开 `Done/Cancelled` 任务详情页

默认进入：`locked`

允许：

- 查看日志
- 查看历史消息
- 查看基础任务信息

不允许：

- `branch-status`
- `diff/ws`
- `open-editor`
- `follow-up`
- 任何会触发 `ensure_container_exists(...)` 的接口

### 行为 2：点击“继续任务”

流程：

1. 弹确认框
2. 后端验证当前任务为 `Done/Cancelled`
3. 后端签发临时 `resume_key`
4. 前端将当前页状态切为 `resumed`
5. 当前页面开始允许危险接口访问

注意：

- 任务状态不变
- 不自动写回 `InProgress`
- 不自动持久化到下次打开

### 行为 3：`resumed` 状态下的允许操作

当前页拿到 `resume_key` 后，允许：

- `branch-status`
- `diff/ws`
- `open-editor`
- `follow-up`

要求：

- 这些接口都必须显式带 `resume_key`
- 后端验证通过后才允许 prepare workspace

### 行为 4：刷新页面/重新进入详情页

页面刷新后：

- 前端内存丢失 `resume_key`
- 页面重新回到 `locked`

这意味着用户必须重新点击“继续任务”。

这是设计上的预期行为，不是 bug。

## 后端职责

后端需要把“危险接口是否允许访问”统一建模，而不是让每个前端组件自己猜。

### 新增接口

建议新增：

- `POST /api/task-attempts/:id/resume`

返回：

- `resume_key`
- `workspace_id`
- `task_id`
- `task_status`

### 后端校验原则

对于 `Done/Cancelled` 任务，下列接口默认拒绝：

- `GET /branch-status`
- `GET /diff/ws`
- `POST /open-editor`
- `POST /sessions/:id/follow-up`

除非：

- 携带有效 `resume_key`

### `resume_key` 要求

- 仅用于短期详情页访问控制
- 不需要长期持久化
- 只需足够区分“本次请求确实来自用户显式 resume 后”
- 可设置较短 TTL（例如 30 分钟以内），但页面刷新即丢失是主要约束

## 前端职责

前端只负责两件事：

1. 正确展示 locked/resumed UI
2. 在危险接口请求里附带 `resume_key`

### 前端状态机

建议页面级状态：

- `open_task`
- `closed_locked`
- `closed_resuming`
- `closed_resumed`

### UI 规则

#### `closed_locked`

显示：

- 只读内容
- “继续任务”按钮

隐藏/禁止挂载：

- follow-up 编辑区
- diff 面板
- preview 中可能触发危险动作的部分
- editor 打开动作

#### `closed_resumed`

显示：

- 正常 follow-up 编辑区
- diff / preview（若当前视图需要）
- 正常 Git 相关辅助信息

但此时任务状态标签仍应显示为：

- `Done` 或 `Cancelled`

## 兼容性分析

### 对现有看板/统计兼容性更好

因为不改 `TaskStatus`：

- 看板列不会跳动
- Done 统计不会失真
- 合并后任务仍保持完成状态
- 历史分析不会被“临时继续查看”污染

### 对误操作控制更好

因为刷新即失效：

- 误切分支只会发生在用户明确点击“继续任务”之后
- 即使用户忘记当前页面已解锁，刷新后也重新回到安全态
- 多标签页之间不会相互污染

### 对系统复杂度可控

相比改任务状态：

- 新增的是访问控制层，而不是重写任务生命周期
- 复杂度集中在“危险接口校验”这一处
- 规则更清晰，后续更容易审计

## 不采用的方案

### 方案一：点击继续任务后改成 `InProgress`

不采用原因：

- 语义错误
- 污染看板
- 兼容性差
- 用户心理模型不一致

### 方案二：只用前端本地布尔值解锁

不采用原因：

- 后端无感知
- 漏洞容易重复出现
- 多入口情况下不稳

### 方案三：resume 状态永久持久化

不采用原因：

- 会变成另一个长期业务状态
- 用户更容易误操作
- 难以解释什么时候自动失效

## 最终推荐

最终推荐模型：

- `TaskStatus` 保持纯业务语义
- `Done/Cancelled` 点击“继续任务”后 **不改状态**
- 当前页面进入临时 `resumed` 访问态
- 危险接口必须带短期 `resume_key`
- 页面刷新/重新进入详情页后恢复 `locked`

这是目前：

- **兼容性最好**
- **出错概率最低**
- **最符合用户心智**

的一种方案。
