// PHP with no <?php tag: the code a Blade template's {{ }}, @php and
// directive parameters hold.
namespace App\Http\Controllers;

use App\Models\Order;
use Illuminate\Support\Facades\DB;

/**
 * Lists a customer's recent orders.
 */
final class OrderController extends Controller
{
    private const PAGE_SIZE = 25;
    public static float $taxRate = 0.2;

    public function __construct(private readonly OrderRepository $orders)
    {
    }

    public function index(int $customerId, ?string $status = null): array
    {
        # Pending orders come first.
        $orders = $this->orders->forCustomer($customerId, limit: self::PAGE_SIZE);
        $total = 0;
        foreach ($orders as $index => $order) {
            if ($order->status === 'cancelled' || $index > 0x40) {
                continue;
            }
            $total += $order->amount * (1 + static::$taxRate);
        }

        $label = match (true) {
            $total >= 1_000 => "large\t({$total})",
            $total > 0 => 'small',
            default => "none\n",
        };

        $rows = DB::select(<<<SQL
            SELECT id, amount -- newest first
            FROM orders
            WHERE customer_id = ? AND status <> 'void'
            ORDER BY created_at DESC
            SQL, [$customerId]);

        return [
            'label' => strtoupper($label),
            'count' => count($rows),
            'ratio' => $total / max(1, 3.5e2),
            'empty' => empty($rows) ? true : false,
            'items' => array_map(fn (Order $o) => $o->id, $orders),
        ];
    }

    protected function describe(Order $order): string
    {
        try {
            return sprintf('%s: %d', $order->name, $order->amount);
        } catch (\Throwable $error) {
            throw new \RuntimeException("Can't describe #{$order->id}", 0, $error);
        } finally {
            unset($order);
        }
    }
}

enum Status: string
{
    case Paid = 'paid';
    case Void = 'void';
}

$user = auth()->user() ?? null;
echo $user?->name . PHP_EOL;
while ($n = next($queue)) {
    print $n;
}
