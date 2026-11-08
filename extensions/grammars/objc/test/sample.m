// A small shape library in Objective-C.
#import <Foundation/Foundation.h>
#include <stdio.h>
#define MAX_SHAPES 16

@import CoreGraphics;

/* Something that can report its area. */
@protocol Measurable <NSObject>
@required
- (double)area;
@optional
- (NSString *)describeWithPrecision:(int)digits;
@end

typedef enum {
    ShapeKindCircle = 0,
    ShapeKindSquare = 0x1,
} ShapeKind;

@interface Shape : NSObject <Measurable> {
@private
    int _identifier;
}
@property (nonatomic, copy) NSString *name;
@property (nonatomic, assign, readonly) ShapeKind kind;
+ (instancetype)shapeWithName:(NSString *)name kind:(ShapeKind)kind;
- (instancetype)initWithName:(NSString *)name kind:(ShapeKind)kind;
@end

@interface Shape (Debugging)
- (void)dump;
@end

static int shapeCount = 0;

static double square(double value) {
    return value * value;
}

@implementation Shape
@synthesize name = _name;

+ (instancetype)shapeWithName:(NSString *)name kind:(ShapeKind)kind {
    return [[self alloc] initWithName:name kind:kind];
}

- (instancetype)initWithName:(NSString *)name kind:(ShapeKind)kind {
    self = [super init];
    if (self != nil) {
        _name = [name copy];
        _kind = kind;
        _identifier = ++shapeCount;
    }
    return self;
}

- (double)area {
    switch (self.kind) {
    case ShapeKindCircle:
        return 3.14159 * square(1.5e0);
    default:
        return square(2.0);
    }
}

- (NSString *)describeWithPrecision:(int)digits {
    return [NSString stringWithFormat:@"%@: %.*f\n", self.name, digits, [self area]];
}
@end

@implementation Shape (Debugging)
- (void)dump {
    printf("shape %d is \"%s\"\n", _identifier, [self.name UTF8String]);
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSMutableArray<Shape *> *shapes = [NSMutableArray array];
        for (int i = 0; i < MAX_SHAPES; i++) {
            ShapeKind kind = (i % 2 == 0) ? ShapeKindCircle : ShapeKindSquare;
            [shapes addObject:[Shape shapeWithName:@"shape" kind:kind]];
        }
        NSDictionary *counts = @{@"total" : @(shapes.count), @"flag" : @YES};
        void (^report)(Shape *) = ^(Shape *shape) {
            NSLog(@"%@", [shape describeWithPrecision:2]);
        };
        @try {
            [shapes enumerateObjectsUsingBlock:^(Shape *obj, NSUInteger idx, BOOL *stop) {
                report(obj);
                *stop = idx >= 3;
            }];
        } @catch (NSException *exception) {
            NSLog(@"failed: %@", exception.reason);
        } @finally {
            [shapes.firstObject dump];
        }
        SEL selector = @selector(area);
        BOOL responds = [shapes.lastObject respondsToSelector:selector];
        id first = counts[@"total"];
        return responds && first != NULL ? 0 : 1;
    }
}
