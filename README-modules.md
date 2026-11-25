# Luau 策略包

比赛只需要**一个入口**，不要求只有一个源文件。可以继续用 `bots/Owl.luau`，也可以把整份策略放进 `bots/Owl/`：

```text
bots/Owl/
  bot.luau             -- 入口；仍负责 @name、@author、@description 和涂装
  core/decision.luau   -- 决策函数
  tactics/drive.luau   -- 可复用的动作提议
```

```luau
-- bot.luau
return require("./core/decision")
```

```luau
-- core/decision.luau
local drive = require("../tactics/drive")
return function(obs)
    return { drive = drive(obs) }
end
```

```luau
-- tactics/drive.luau
return function(obs)
    return if obs.enemy_ahead then "stop" else "forward"
end
```

从车库选一次 `Owl` 就能出战。不需要打包、拼接源码或改写 `require`。

- `./` 和 `../` 相对于**写下 `require` 的模块**，可省略文件的 `.luau` 后缀。模块可返回函数、表或其他值；一个模块在同一席位只初始化一次，各席位不共享可变状态。
- 开局时读取包内各层目录的 `.luau` 普通文件，跳过点文件、点目录和符号链接。`require` 只查这份快照，不查宿主目录。比赛中编辑模块会在下次开局生效，回放摘要覆盖模块相对路径和内容。
- 只有目录根的 `bot.luau` 是选手；顶层单文件不把其他车型当作依赖。找不到模块和循环依赖的报错会指明涉及的模块。

## Avalon 模块候选

**独立留档：`Avalon-Pivot`。** 车库中的 `bots/Avalon-Pivot/` 是 `pivotLocal` 的可选快照，源码在 `assets/strategies/avalon-pivot/`。从 A 轮封存包提取，只改了入口的 `@name`，不替换当前模块候选或单文件 Avalon。

**后续 Avalon 的策略改动都以 `assets/strategies/avalon-modular/` 为源码。** 它是完整的参赛包，不是生成器。`bots/Avalon-Modular/` 是车库中的部署副本，不在两处分别修改。更新车库时同步整包，选择 **Avalon·Prefix**。原有 `bots/Avalon.luau` 保留为冻结对照，不作为训练写入或自动覆盖的目标。

| 修改目的 | 从哪里入手 |
| --- | --- |
| 改变一帧中各层的调用顺序或最终输出 | `decision.luau` |
| 修改观察结构、每局常量、几何查询或标定参数 | `model/` |
| 改变跟踪、选敌或威胁排序 | `perception/tracks.luau`、`perception/situation.luau` |
| 改变射击窗口、移动提议或路径选择 | `tactics/fire.luau`、`tactics/movement.luau`、`tactics/navigation.luau` |
| 改变风险估算、闪避选择或近身复核 | `safety/risk.luau`、`safety/evasion.luau`、`safety/maneuver.luau` |

运行 `cargo test --test modules` 检查模块加载及候选在固定回放观察上的动作，不开新比赛。引擎和重新编译的训练 worker 都能读取包入口；现有按单文件生成训练变体的 Python 流程尚未迁移。

### 当前 60 步候选：`prefix`

车库的 **Avalon·Prefix** 已从 `all60` 更新为 `prefix`；这是 `Avalon-Pivot` 的后继，不是它的改名。当前策略把**实弹硬碰撞**的检查保留到 60 个游戏 tick（20 段 × 3 tick），不因弹幕密度缩短；已经撞到、已消失或没有威胁的路径不必继续。远期的碰撞不再等于眼前就要避让：在有战术目标且不受近身保护约束时，先验证「走一段再刹停」或「转一段再拉直」的续段，整段足够安全才保留当前动作，下次观察重新作决定。

**走位收益**与**硬碰撞**的时间窗现在分开：密集局面可用第 9 步姿态评估，多弹、转向时可用第 24 步，稳定时用到第 60 步；团队中的速度外推会在对方改变运动时缩短，稳定后恢复至 60 步。上限不是 VM 配额、按键必须保持的时长或引擎模拟次数。开火预测、贴身复核与局部收益均不超过 60 步。

独立终验、原始计分及复算（本地档案：`training/avalon/evidence/modular/intent-budget60-2950000/报告.md`）：综合真胜率 `all60` 50.61%、历史单文件 58.16%、`prefix` 63.88%。团队接战、停滞、脱险回压、超时同时通过对现役和历史的事前风格门槛；狭义的必须先绕路机会仍比历史少转化 5.20 个百分点，未宣称所有战术行为一致。

旧线各阶段：固定 all60（本地档案：`training/avalon/evidence/modular/window60-2840000/报告.md`） · 16 种分层预算（本地档案：`training/avalon/evidence/modular/architecture60-2850000/报告.md`） · 父代 pivotLocal 的主动性验收（本地档案：`training/avalon/evidence/modular/initiative60-2930000/报告.md`）。
