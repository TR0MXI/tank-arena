--!strict
local Corner = require("./safety/corner")
local Fire = require("./tactics/fire")
local Frame = require("./model/frame")
local Geo = require("./model/geometry")
local Risk = require("./safety/risk")
local Safety = require("./safety/maneuver")
local Situation = require("./perception/situation")
local Tactics = require("./tactics/movement")
local Tracks = require("./perception/tracks")
local evade = require("./safety/evasion")
local Types = require("./model/types")

type Obs = Types.Obs

local priorDrive: string?, priorTurn: string?, driveAt, turnAt = nil, nil, 0, 0
@native
local function decide(o: Obs)
    Frame.update(o)
    local me = o.me
    local now = o.tick

    Tracks.update(o, now)
    Risk.chooseClearance(o, me)
    local target, aimX, aimY, ally, threat, guards, tightest, changed = Situation.acquire(o, me)
    if changed then
        Tactics.targetChanged()
    end
    local victim, flight = Fire.window(o, me)
    local fire = victim ~= nil
    local drive, turn = "stop", "none"
    if target then
        drive, turn = Tactics.attack(o, me, target, aimX, aimY, ally, now, victim == target.slot_id, guards)
        drive, turn = Tactics.unstick(o, me, now, drive, turn)
    end

    -- 知道对方在墙后不等于正受炮口威胁；只在近距离直射线畅通时提前避让。
    local warning = Risk.prefire(o, me, threat)
    if not warning and not Tactics.recovering(now) and threat and math.abs(Geo.errorTo(me, threat.x, threat.y)) > 10 then
        local angle = me.heading * math.pi / 128
        local aim = threat.heading * math.pi / 128
        local side = math.sin(angle - aim)
        local offset = math.cos(aim) * (me.y - threat.y) - math.sin(aim) * (me.x - threat.x)
        local d = if offset * side >= 0 then "forward" else "reverse"
        local lead = if d == "forward" then 240 else -180
        if not Geo.cast(me.x, me.y, math.cos(angle) * lead, math.sin(angle) * lead, Frame.hull) then
            drive = d
        end
    end
    drive = Corner.adjust(o, me, drive, turn)
    drive, turn = Safety.keepRoom(o, me, drive, turn, guards, tightest)

    local hazards
    if #o.shells > 0 or warning then
        local paths, ownReturn = Risk.forecast(o, me)
        if warning and #paths < 8 then
            table.insert(paths, warning)
        end
        fire = fire and (not ownReturn or (o.enemy_ahead and flight ~= nil and flight <= 12))
        if #paths > 0 then
            Risk.prepareRisk(o, me, paths)
            paths.guards = guards
            hazards = paths
        end
    end
    drive, turn = evade(o, hazards, target, drive, turn, driveAt, turnAt)
    -- 只延长九步内即将折返的那根轴；
    -- 有弹时需重新证明延长后的动作安全，拥挤弹幕直接让避弹层拍板。
    local pulseD = not Safety.spacing() and drive == priorDrive and drive ~= o.drive and now - driveAt <= 9
    local pulseT = turn == priorTurn and turn ~= o.turn and now - turnAt <= 9
    if (pulseD or pulseT) and not Tactics.recovering(now) then
        local d, t = if pulseD then o.drive else drive, if pulseT then o.turn else turn
        if
            not hazards
            or (
                #hazards <= 4
                and Risk.score(o, me, hazards, d, t) <= math.min(12000, Risk.score(o, me, hazards, drive, turn) + 800)
            )
        then
            drive, turn = d, t
        end
    end
    drive, turn = Safety.breakContact(o, me, hazards, target, drive, turn)
    drive, turn = Safety.nearEscape(o, me, hazards, drive, turn)
    if drive ~= o.drive then
        priorDrive, driveAt = o.drive, now
    end
    if turn ~= o.turn then
        priorTurn, turnAt = o.turn, now
    end
    if fire and victim and flight then
        Fire.record(o, victim, flight)
    end
    -- 不重复按键；射后必须显式松开扳机。
    return {
        drive = if drive ~= o.drive then drive else nil,
        turn = if turn ~= o.turn then turn else nil,
        fire = if fire then true elseif o.fire_down then false else nil,
    }
end

return decide
