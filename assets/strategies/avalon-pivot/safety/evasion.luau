--!strict
local Risk = require("./risk")
local Safety = require("./maneuver")
local Tracks = require("../perception/tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

local evadeD: string?, evadeT: string, holdUntil = nil, "none", 0

-- 在已有移动提议上择优；没有危险时，清掉上一次的闪避承诺。
@native
local function choose(o: Types.Obs, paths: Types.Paths?, target: Types.Tank?, drive: string, turn: string,
    driveAt: number, turnAt: number): (string, string)
    if not paths then
        evadeD = nil
        return drive, turn
    end
    local me, now = o.me, o.tick
    local committed = if now < holdUntil then evadeD else nil
    -- Preserve the full collision horizon. Local posture becomes less predictable in a crowd.
    if paths.guards or #paths >= 3 then paths.poseSlice = 3
    elseif #paths > 1 or o.turn ~= "none" then paths.poseSlice = 8 end
    local nominal = Risk.score(o, me, paths, drive, turn)
    if nominal <= 850 then
        -- 危险边缘的评分会跨过零线。在两条路线都安全时把已开始的闪身走完几步。
        if committed and Risk.score(o, me, paths, committed, evadeT) <= math.min(nominal + 800, 1800) then
            drive, turn = committed, evadeT
        else
            evadeD = nil
        end
    else
        -- 同一时刻比较双方位置，并使用推演实际得到的朝向（贴墙/拉直时尤其重要）。
        local tx, ty, before = 0, 0, 0
        if target then
            tx, ty = Tracks.predict(target, (paths.poseSlice or paths.steps) * Tuning.SLICE)
            before = math.sqrt((me.x - tx) ^ 2 + (me.y - ty) ^ 2)
        end
        local function value(d: string, r: string, straighten: number?)
            local danger, x, y, c, s = Risk.score(o, me, paths, d, r, straighten)
            if not target or danger >= 500000 then
                return danger -- 撞弹是终局，不奖给这条路线虚构的未来位置。
            end
            local dx, dy = tx - x, ty - y
            local angle = math.abs(math.atan2(c * dy - s * dx, c * dx + s * dy)) * 128 / math.pi
            local progress = if paths.guards then 0 else math.clamp(math.sqrt(dx * dx + dy * dy) - before, -320, 640)
            return danger + 2 * angle + 0.45 * progress
        end
        local bestD, bestT, best = drive, turn, value(drive, turn)
        if committed then
            local stay = value(committed, evadeT)
            if stay <= best + 1600 then
                bestD, bestT, best = committed, evadeT, stay - 1600
            end
        end
        if best > 0 then
            for _, d in Safety.drives do
                for _, r in Safety.turns do
                    if (d == "stop" and r ~= "none") or (d == drive and r == turn) then
                        continue
                    end
                    local score = value(d, r) + 12
                    -- 给正在生效的按键一个连续性优势；面对实弹命中（50 万起）可以当场换向。
                    if d ~= o.drive then
                        score += (if now - driveAt <= 8 then 2200 else 450)
                    end
                    if r ~= o.turn then
                        score += (if now - turnAt <= 8 then 1500 else 260)
                    end
                    if d == "stop" then
                        score += 280
                    end
                    if
                        r ~= "none"
                        and d ~= "stop"
                        and score >= 500000
                        and #paths <= 4
                    then
                        -- 另一种可行逃生路线：先拐弯再直走，不假设会在原地一直绕圈。
                        local _, _, _, _, _, hit = Risk.score(o, me, paths, d, r)
                        local bend = math.clamp(math.floor(hit / (2 * Tuning.SLICE)) + 1, 3, 8)
                        score = math.min(score, value(d, r, bend) + 24)
                    end
                    -- 小幅风险差先走完这段弧线；真实撞击的分层评分远高于换键成本。
                    if committed and (d ~= evadeD or r ~= evadeT) then
                        score += 900
                    end
                    if score < best then
                        bestD, bestT, best = d, r, score
                    end
                end
            end
        end
        if bestD ~= drive or bestT ~= turn then
            if not committed or bestD ~= evadeD or bestT ~= evadeT then
                holdUntil = now + Tuning.COMMIT
            end
            drive, turn = bestD, bestT
            evadeD, evadeT = drive, turn
        else
            evadeD = nil
        end
    end
    return drive, turn
end

return choose
