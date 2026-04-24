function App() {
  const [n, setN] = React.useState<number>(0);
  return (
    <>
      <button onClick={() => setN((x) => x + 1)}>+</button>
      <span>{n}</span>
      <button onClick={() => setN((x) => x - 1)}>-</button>
    </>
  );
}
