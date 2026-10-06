interface Props {
  title: string;
  count?: number;
}

enum Mode {
  Light,
  Dark,
}

class Counter extends React.Component<Props> {
  state = { count: 0 };

  render() {
    return <div>{this.state.count}</div>;
  }
}

export const theme = Mode.Dark;

export function App(props: Props) {
  const label = props.title;
  return <Counter title={label} />;
}
