#include <string>

#define MAX_ITEMS 10
#define SQUARE(x) ((x) * (x))

namespace shapes {

const int kSides = 4;
int counter = 0, *cursor;
int area_of(int sides);

struct Point {
    int x, y;
    double length() const;
};

class Shape {
public:
    explicit Shape(int sides) : sides_(sides) {}
    virtual ~Shape() {}

    int area() const {
        int local = sides_ * 2;
        return local;
    }

    template <typename T>
    T scaled(T factor) const { return factor; }

    static int count;
    Shape **neighbours();
    operator char *() const;

private:
    int sides_;
    int (*callback_)(int);
};

enum class Color { Red, Green };

union Value {
    int i;
    float f;
};

typedef unsigned long Size;
using Name = std::string;

template <typename T>
T maximum(T a, T b) {
    return a > b ? a : b;
}

template <typename T>
T zero = T();

namespace detail {
void helper(int param) {}
int twice(int value);
}  // namespace detail

int detail::twice(int value) { return value * 2; }

double Point::length() const { return 0; }

int Shape::count = 0;

Shape **Shape::neighbours() { return nullptr; }

Shape::operator char *() const { return nullptr; }

}  // namespace shapes

int main(int argc, char **argv) {
    int result = shapes::maximum(argc, 1);
    return result;
}
