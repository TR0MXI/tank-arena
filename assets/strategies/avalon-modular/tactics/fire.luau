--!strict
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tracks = require("../perception/tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Obs = Types.Obs
type Tank = Types.Tank

-- 用炮口到目标的飞行时间而不是车心距离估算提前量。有限时域内允许打横穿的敌车。
@native
local function firingSolution(o: Obs, me: Tank, t: Tank, aimX: number?, aimY: number?): (number?, number?)
    if not aimX or not aimY then
        return nil, nil
    end
    local m = Tracks.bySlot[t.slot_id]
    if m.vx == 0 and m.vy == 0 then
        return aimX, aimY
    end
    local dx, dy = t.x - me.x, t.y - me.y
    local flight = math.max(0, (math.sqrt(dx * dx + dy * dy) - Frame.muzzle) / o.shell_speed)
    local x, y = Tracks.predict(t, flight)
    local turnTime = math.abs(Geo.errorTo(me, x, y)) / Frame.spin
    dx, dy = x - me.x, y - me.y
    x, y = Tracks.predict(t, math.max(0, (math.sqrt(dx * dx + dy * dy) - Frame.muzzle) / o.shell_speed) + turnTime)
    x, y = aimX + x - t.x, aimY + y - t.y
    if (x ~= aimX or y ~= aimY) and not Geo.direct(me.x, me.y, x, y, Frame.shell) then
        x, y = aimX, aimY
    end
    return x, y
end

-- 以相对运动检查这一段预计先碰到谁。队友的安全余量更大；敌车须命中车身内部。
@native
local function firstBody(
    o: Obs,
    me: Tank,
    x: number,
    y: number,
    dx: number,
    dy: number,
    start: number,
    dt: number,
    returning: boolean
): (number, Tank?)
    local best, victim: Tank? = 2, nil
    for _, t in o.tanks do
        if not t.alive or (not returning and t.slot_id == me.slot_id) then
            continue
        end
        local m = Tracks.bySlot[t.slot_id]
        local ally = t.team_id == me.team_id
        local r = if ally
            then math.max(o.hit_radius, o.turret_offset + o.turret_half_length + Frame.shell)
            else o.hit_radius - 28
        local stop = Tracks.coastTime(t, m)
        local from = math.min(stop, start)
        local px, py = t.x + m.vx * from, t.y + m.vy * from
        local moving = math.clamp(stop - start, 0, dt)
        local hit
        if dt > 0 and moving > 0 and moving < dt then
            -- 前半段相对运动，碰墙/达到外推上限后车停而弹不停。两段命中时间统一成比例。
            local f = moving / dt
            hit = Geo.slab(x, y, dx * f - m.vx * moving, dy * f - m.vy * moving, px - r, py - r, px + r, py + r)
            if hit then
                hit *= f
            else
                px, py = px + m.vx * moving, py + m.vy * moving
                hit = Geo.slab(x + dx * f, y + dy * f, dx * (1 - f), dy * (1 - f), px - r, py - r, px + r, py + r)
                if hit then
                    hit = f + hit * (1 - f)
                end
            end
        elseif not ally and moving == 0 and o.enemy_ahead and not returning then
            -- 贴身静止时常被长车头互相卡住。真实炮线已畅通，按旋转后的底盘和炮塔取先到者，
            -- 不把长方形车身缩成正方形而错过侧面的解围机会。
            local ux, uy = Geo.vector(t.heading, 1)
            local rx, ry = x - px, y - py
            local lx, ly = rx * ux + ry * uy, ry * ux - rx * uy
            local vx, vy = dx * ux + dy * uy, dy * ux - dx * uy
            local hull = Geo.slab(lx, ly, vx, vy, -o.hull_half_length, -Frame.hull, o.hull_half_length, Frame.hull)
            local turret = Geo.slab(
                lx,
                ly,
                vx,
                vy,
                o.turret_offset - o.turret_half_length,
                -o.turret_half_width,
                o.turret_offset + o.turret_half_length,
                o.turret_half_width
            )
            hit = if hull and turret then math.min(hull, turret) else hull or turret
        else
            hit = Geo.slab(x, y, dx - m.vx * moving, dy - m.vy * moving, px - r, py - r, px + r, py + r)
        end
        if hit and (hit < best or (hit == best and ally)) then
            best, victim = hit, t
        end
    end
    return best, victim
end

-- 校验当前朝向，而非假设炮塔可以瞬间转向。第一段允许打横穿，弹药充裕时再试一次反弹。
local nextSalvo: { number }, lastBearing: { number } = {}, {}
@native
local function shotWindow(o: Obs, me: Tank): (number?, number?)
    if not o.can_fire then
        return nil, nil
    end
    -- 开火先于本步驾驶。最后一发只用于近身直射，不能因 enemy_ahead 为真越过储备线。
    local dx, dy = Geo.vector(me.heading, Frame.muzzle)
    local x, y = me.x + dx, me.y + dy
    local vx, vy = Geo.vector(me.heading, o.shell_speed)
    local range = if o.ammo == 1 then 1.4 elseif o.enemy_ahead then 4.5 else 2.5
    local elapsed, remaining = 0, Tuning.HORIZON_TICKS
    for leg = 1, 2 do
        dx, dy = vx * remaining, vy * remaining
        local at, fx, fy = Geo.cast(x, y, dx, dy, Frame.shell, true)
        local hit, victim = firstBody(o, me, x, y, dx, dy, elapsed, remaining, leg == 2)
        if victim and hit <= (at or 1) then
            if victim.team_id == me.team_id or hit == at then
                return nil, nil
            end
            local flight = elapsed + hit * remaining
            if flight * o.shell_speed > range * Frame.cell then
                return nil, nil -- 射程和弹药策略只决定是否出手，不决定预测窗口。
            end
            -- 已经切进短弹程空当，不必等上一次远射的补射计时走完。
            local oldBearing = lastBearing[victim.slot_id] or me.heading
            local spread = math.abs((me.heading - oldBearing + 128) % 256 - 128)
            if o.tick < (nextSalvo[victim.slot_id] or 0) and flight > 10 and (spread < 15 or o.ammo <= 2) then
                return nil, nil
            end
            -- 长弹程给对方充足横移时间；弹槽不多时，不为同一条远炮线倾空弹匣。
            if o.ammo <= 2 and flight > 40 then
                return nil, nil
            end
            -- 补射按弹程和剩余弹槽定价，不耽误对新目标的首发；久未交火则重新放开。
            local readyAt = nextSalvo[victim.slot_id]
            if readyAt and o.tick < readyAt + 240 then
                local limit = if o.ammo >= 4 then 48 elseif o.ammo == 3 then 30 else 18
                if flight > limit then
                    return nil, nil
                end
            end
            return victim.slot_id, flight
        end
        if leg == 2 or not at or o.ammo < 2 then
            break
        end
        local dt = at * remaining
        x, y = x + at * dx, y + at * dy
        local dot = 2 * (vx * fx + vy * fy)
        vx, vy = vx - dot * fx, vy - dot * fy
        x, y = x + math.sign(vx) * 0.0001, y + math.sign(vy) * 0.0001
        elapsed, remaining = elapsed + dt, remaining - dt
    end
    return nil, nil
end

local function readyAt(slot: number): number
    return nextSalvo[slot] or 0
end

local function record(o: Obs, victim: number, flight: number)
    lastBearing[victim] = o.me.heading
    nextSalvo[victim] = o.tick + math.clamp(flight * (if o.ammo >= 4 then 0.72 else 1.15), 10, 78)
end

return {
    firingSolution = firingSolution,
    window = shotWindow,
    readyAt = readyAt,
    record = record,
}
