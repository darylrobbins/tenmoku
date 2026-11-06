// An undo stack with merging, used as a golden highlighting sample.
#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace golden {

struct Edit {
    std::size_t offset = 0;
    std::string removed;
    std::string inserted;
};

template <typename T>
concept Mergeable = requires(T a, const T& b) {
    { a.merge(b) } -> std::same_as<bool>;
};

class Transaction final {
public:
    explicit Transaction(Edit edit) : edits_{std::move(edit)} {}
    Transaction(const Transaction&) = delete;
    Transaction(Transaction&&) noexcept = default;
    virtual ~Transaction() = default;

    bool merge(const Transaction& next) {
        auto& last = edits_.back();
        const auto& first = next.edits_.front();
        if (last.offset + last.inserted.size() != first.offset) {
            return false;
        }
        last.inserted += first.inserted;
        return true;
    }

    [[nodiscard]] std::size_t size() const noexcept { return edits_.size(); }

private:
    std::vector<Edit> edits_;
};

template <Mergeable T>
class UndoStack {
public:
    void push(T item) {
        if (!undo_.empty() && undo_.back()->merge(item)) {
            return;
        }
        undo_.push_back(std::make_unique<T>(std::move(item)));
        redo_.clear();
    }

    std::optional<T*> undo() {
        if (undo_.empty()) return std::nullopt;
        redo_.push_back(std::move(undo_.back()));
        undo_.pop_back();
        return redo_.back().get();
    }

    static constexpr int kLimit = 1'000;

private:
    std::vector<std::unique_ptr<T>> undo_, redo_;
};

}  // namespace golden

int main() {
    using namespace golden;
    UndoStack<Transaction> stack;
    stack.push(Transaction{Edit{0, "", "hello"}});
    auto* raw = new int[4]{1, 2, 3, 4};
    delete[] raw;
    std::printf("%d items, %s\n", 4, "ok");
    return stack.undo().has_value() && nullptr == nullptr ? 0 : 1;
}
