package demo;

public class Account {
    public static final int LIMIT = 10;
    private String owner, bank;
    private int[] history;

    public Account(String owner) {
        this.owner = owner;
        Runnable r = new Runnable() {
            int hidden;

            public void run() {}
        };
    }

    public int balance() {
        int local = 0;
        return local;
    }

    enum Status {
        OPEN,
        CLOSED {
            int extra;
        };

        private final int code = 0;
    }

    interface Limits {
        int MAX = 100;
    }

    record Pair(int left, int right) {
        static int count;
    }
}
