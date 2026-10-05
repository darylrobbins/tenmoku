package demo;

import java.util.List;

public class Shapes {
    private final List<String> names;

    public Shapes(List<String> names) {
        this.names = names;
    }

    public int count() {
        return names.size();
    }

    interface Drawable {
        void draw();
    }

    enum Kind {
        ROUND,
        SQUARE;

        boolean isRound() {
            return this == ROUND;
        }
    }

    record Point(int x, int y) {
        int sum() {
            return x + y;
        }
    }
}

@interface Marker {
    String value();
}
