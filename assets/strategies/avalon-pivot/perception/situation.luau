--!strict
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tracks = require("./tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Guard = Types.Guard
type Obs = Types.Obs
type Tank = Types.Tank

local focus: number?
-- 墙角可能挡住车心却没挡住整辆车。试瞄两侧的车身，不因此丢掉追击目标。
@native
local function aimPoint(me: Tank, t: Tank, dx: number, dy: number, d2: number): (number?, number?)
    if d2 > (6 * Frame.cell) ^ 2 then
        return nil, nil -- 远距离先沿通道接近，实际炮口已有直射机会时仍可开火。
    end
    if Geo.direct(me.x, me.y, t.x, t.y, Frame.shell) then
        return t.x, t.y
    end
    local s = (Frame.hull - 12) / math.max(1, math.abs(dx), math.abs(dy))
    for side = -1, 1, 2 do
        local x, y = t.x - side * dy * s, t.y + side * dx * s
        if Geo.direct(me.x, me.y, x, y, Frame.shell) then
            return x, y
        end
    end
    return nil, nil
end

-- 选人使用整张地图；近距离畅通的炮线只提高交战优先级，不是发现条件。

-- 算的是「敌方现在可以开火，自己还剩几步横移」而不是固定追击半径。
-- 正面相持要先转动车身；已在横切、或敌方尚需大幅转炮时，可以打得更近。
@native
local function reactionRoom(o: Obs, me: Tank, t: Tank, dx: number, dy: number, range: number, offset: number)
    local ux, uy = dx / range, dy / range
    local hx, hy = Geo.vector(me.heading, 1)
    local axial = math.abs(hx * ux + hy * uy)
    local sweep = (Frame.hull + Frame.shell) / o.forward_speed + axial * 40 / Frame.spin
    local theirTurn = math.max(0, offset - 8) / Frame.spin
    local m = Tracks.bySlot[t.slot_id]
    local rush = math.clamp(-(m.vx * ux + m.vy * uy), 0, o.forward_speed)
    -- 观察到敌方消耗弹槽且我方有弹时，缩短预留的转身时间，抢回进攻距离。
    -- 空仓将回弹、初见缺弹、我方无弹：仍保留原来的反应空间。
    local press = if m.firedAt
            and o.tick - m.firedAt < 400
            and m.loaded > 0
            and m.loaded < Frame.capacity
            and o.ammo > 0
        then 25
        else 0
    local body = Frame.hull + axial * (o.turret_offset + o.turret_half_length - Frame.hull)
    local floor = Frame.muzzle
        + body
        + Frame.shell
        + o.shell_speed * math.max(0, 6 + Frame.cadence + sweep - theirTurn - press)
        + rush * 8
    return range - floor
end

@native
local function acquire(o: Obs, me: Tank): (Tank?, number?, number?, boolean, Tank?, { Guard }?, Guard?, boolean)
    local target: Tank?, best, aimX: number?, aimY: number?, screened = nil, math.huge, nil, nil, false
    local threat: Tank?, urgency = nil, -math.huge
    local guards: { Guard }?, tightest: Guard?
    for _, t in o.tanks do
        if not t.alive or t.team_id == me.team_id then
            continue
        end
        local dx, dy = t.x - me.x, t.y - me.y
        local d2 = dx * dx + dy * dy
        local x, y = aimPoint(me, t, dx, dy, d2)
        local range = math.sqrt(d2)
        local offset = if x then math.abs(Geo.errorTo(t, me.x, me.y)) else 128
        -- 只在车心直射线开通时留距；墙角仅能瞄到车边时仍按原路线探出。
        if x == t.x and y == t.y and range > 0 and range < 2.5 * Frame.cell then
            local m = Tracks.bySlot[t.slot_id]
            -- 弹的剩余寿命是公开信息。最先到期的那发给出回槽时间上界，撞车销弹还会更早。
            local readyIn = if m.loaded > 0 then 0 else m.refill
            local room = math.huge
            if offset < 48 and readyIn <= Tuning.HORIZON_TICKS then
                room = reactionRoom(o, me, t, dx, dy, range, offset) + math.max(0, readyIn - 18) * Frame.reverse
            end
            -- 自己也空仓时，两边很快就会先后回弹，不能赌车头已经对正；我方明显先回弹时仍可抢近线。
            if o.ammo == 0 and m.loaded == 0 then
                if Frame.solo and readyIn <= Tuning.HORIZON_TICKS and readyIn <= Tracks.bySlot[me.slot_id].refill + 20 then
                    -- 逼近中的敌车不会等在原地。预留一个短转向窗口，而非静态禁入圈。
                    local rush = math.clamp(-(dx * m.vx + dy * m.vy) / range, 0, o.forward_speed)
                    room = math.min(room, range - 1.24 * Frame.cell - rush * Tuning.COMMIT)
                elseif readyIn <= 36 and offset < 64 then
                    -- 多车交战时，把拉距留给即将开火的近炮，别为了一个远期回槽放弃队友的射线。
                    room = math.min(room, range - 1.05 * Frame.cell)
                end
            end
            if room < 0.35 * Frame.cell then
                local g: Guard = { t = t, room = room, range = range }
                guards = guards or {}
                table.insert(guards, g)
                if not tightest or room < tightest.room then
                    tightest = g
                end
            end
        end
        local ally, wing = false, false
        if x and y then
            dx, dy = x - me.x, y - me.y
            local len2 = math.max(1, dx * dx + dy * dy)
            for _, a in o.tanks do
                if not a.alive or a.team_id ~= me.team_id or a.slot_id == me.slot_id then
                    continue
                end
                local p = ((a.x - me.x) * dx + (a.y - me.y) * dy) / len2
                if
                    p > 0
                    and p < 1
                    and (a.x - me.x - p * dx) ^ 2 + (a.y - me.y - p * dy) ^ 2 < (o.hit_radius + 65) ^ 2
                then
                    ally = true
                    break
                end
            end
        end
        local score = range
            - (if x and y and not ally then 2 * Frame.cell else 0)
            - (if focus == t.slot_id then 350 else 0)
        if x and y and not ally then
            -- 多个射击窗口并存时，先压制能迅速瞄准的敌车，尤其是正瞄着我方的目标。
            score += math.abs(Geo.errorTo(me, x, y)) * 5
            if Tracks.armed(t) then
                if offset < 16 then
                    score -= 240
                end
                local danger = -math.sqrt(d2) - offset * 22
                if offset < 28 and d2 < (2.3 * Frame.cell) ^ 2 and danger > urgency then
                    threat, urgency = t, danger
                end
            end
            -- 队友已经对准的敌车是天然的夹击目标；仅用公开朝向和通道，不交换私有状态。
            local supported = false
            for _, f in o.tanks do
                if
                    f.alive
                    and f.team_id == me.team_id
                    and f.slot_id ~= me.slot_id
                    and (Tracks.armed(f) or Tracks.bySlot[f.slot_id].refill <= Tuning.COMMIT)
                    and (t.x - f.x) ^ 2 + (t.y - f.y) ^ 2 < (3 * Frame.cell) ^ 2
                    and math.abs(Geo.errorTo(f, t.x, t.y)) < 18
                    and Geo.direct(f.x, f.y, t.x, t.y, Frame.shell)
                then
                    supported = true
                    -- 同一翼已有更靠前的队友架炮。后车打出开场齐射，再切线，不跟着挤同一个门。
                    local ax, ay = t.x - f.x, t.y - f.y
                    if
                        o.ammo <= Frame.capacity - 2
                        and ax * ax + ay * ay < d2
                        and ax * dx + ay * dy > 0
                        and math.abs(ax * dy - ay * dx) < 0.55 * Frame.cell * math.sqrt(d2)
                        and (me.x - f.x) ^ 2 + (me.y - f.y) ^ 2 < (1.8 * Frame.cell) ^ 2
                    then
                        wing = true
                    end
                    if wing then
                        break
                    end
                end
            end
            if supported then
                Tracks.bySlot[t.slot_id].covered = true
                score -= 380
            end
        end
        if score < best then
            target, best, aimX, aimY, screened = t, score, x, y, ally or wing
        end
    end
    local nextFocus = if target then target.slot_id else nil
    local changed = nextFocus ~= focus
    focus = nextFocus
    return target, aimX, aimY, screened, threat, guards, tightest, changed
end

return {
    acquire = acquire,
}
