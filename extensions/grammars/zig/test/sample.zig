//! A small stack and a word counter, to exercise the highlighter.
const std = @import("std");
const Allocator = std.mem.Allocator;

/// Errors a stack can report.
const StackError = error{ Empty, Overflow };

const Color = enum(u8) {
    red,
    green = 2,
    blue,

    pub fn isWarm(self: Color) bool {
        return self == .red;
    }
};

const Shape = union(enum) {
    circle: f32,
    rect: struct { w: f32, h: f32 },
};

pub fn Stack(comptime T: type) type {
    return struct {
        const Self = @This();

        items: []T,
        len: usize = 0,
        allocator: Allocator,

        pub fn init(allocator: Allocator, capacity: usize) !Self {
            const items = try allocator.alloc(T, capacity);
            return Self{ .items = items, .allocator = allocator };
        }

        pub fn deinit(self: *Self) void {
            self.allocator.free(self.items);
        }

        pub fn push(self: *Self, value: T) StackError!void {
            if (self.len == self.items.len) return error.Overflow;
            self.items[self.len] = value;
            self.len += 1;
        }

        pub fn pop(self: *Self) ?T {
            if (self.len == 0) return null;
            self.len -= 1;
            return self.items[self.len];
        }
    };
}

fn area(shape: Shape) f32 {
    return switch (shape) {
        .circle => |r| 3.14159 * r * r,
        .rect => |r| r.w * r.h,
    };
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var stack = try Stack(i32).init(allocator, 8);
    defer stack.deinit();

    const mask: u32 = 0xFF_00 | 0b1010 | 0o17;
    var i: i32 = 0;
    while (i < 5) : (i += 1) {
        try stack.push(i * 2);
    }
    while (stack.pop()) |value| {
        std.debug.print("value = {d}\tmask = {x}\n", .{ value, mask });
    }

    const greeting = "Hello, \"Zig\"!\n";
    const initial = 'z';
    const big = 1.5e3;
    for (greeting, 0..) |c, n| {
        if (c == initial or n > 3) break;
    }
    _ = big;
    _ = area(.{ .circle = 2.0 });
}

test "stack pushes and pops" {
    var stack = try Stack(u8).init(std.testing.allocator, 2);
    defer stack.deinit();
    try stack.push(1);
    try std.testing.expectEqual(@as(?u8, 1), stack.pop());
}
