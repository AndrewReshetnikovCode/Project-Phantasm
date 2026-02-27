function on_init()
    local room_w = 40
    local room_h = 15

    for x = 0, room_w - 1 do
        for y = 0, room_h - 1 do
            if x == 0 or x == room_w - 1 or y == 0 or y == room_h - 1 then
                cmd_spawn({
                    Position = {x = x, y = y},
                    Glyph = {ch = "#", fg = "DarkGrey", bg = "Black"},
                    Collider = {solid = true},
                    Wall = {}
                })
            end
        end
    end

    cmd_spawn({
        Position = {x = 5, y = 5},
        Glyph = {ch = "@", fg = "Yellow", bg = "Black"},
        Player = {},
        Name = {name = "Hero"}
    })

    cmd_spawn({
        Position = {x = 15, y = 7},
        Glyph = {ch = "$", fg = "BrightYellow", bg = "Black"},
        Name = {name = "Gold Coin"},
        Item = {type = "gold", value = 10}
    })

    cmd_spawn({
        Position = {x = 25, y = 10},
        Glyph = {ch = "!", fg = "BrightRed", bg = "Black"},
        Name = {name = "Health Potion"},
        Item = {type = "potion", value = 25}
    })

    cmd_spawn({
        Position = {x = 30, y = 5},
        Glyph = {ch = "N", fg = "BrightCyan", bg = "Black"},
        Name = {name = "Old Sage"},
        NPC = {dialogue = "Welcome to Phantasm!"}
    })

    cmd_log("Hello Phantasm! World initialized with dungeon room.")
end

function on_update(dt)
    if not _input or not _input.pressed then return end

    local players = query("Position", "Player")
    for _, p in ipairs(players) do
        local nx = p.Position.x
        local ny = p.Position.y

        if _input.pressed["move_up"] then ny = ny - 1 end
        if _input.pressed["move_down"] then ny = ny + 1 end
        if _input.pressed["move_left"] then nx = nx - 1 end
        if _input.pressed["move_right"] then nx = nx + 1 end

        local blocked = false
        local walls = query("Position", "Collider")
        for _, wall in ipairs(walls) do
            if wall.Position.x == nx and wall.Position.y == ny and wall.Collider.solid then
                blocked = true
                break
            end
        end

        if not blocked and (nx ~= p.Position.x or ny ~= p.Position.y) then
            cmd_set(p.id, "Position", {x = nx, y = ny})

            local items = query("Position", "Item", "Name")
            for _, item in ipairs(items) do
                if item.Position.x == nx and item.Position.y == ny then
                    cmd_log("Picked up: " .. item.Name.name)
                    cmd_despawn(item.id)
                end
            end

            local npcs = query("Position", "NPC", "Name")
            for _, npc in ipairs(npcs) do
                if math.abs(npc.Position.x - nx) <= 1 and math.abs(npc.Position.y - ny) <= 1 then
                    cmd_log(npc.Name.name .. ": " .. npc.NPC.dialogue)
                end
            end
        end
    end
end
