import { Link } from 'react-router-dom'
import styles from './Header.module.css'

export default function Header() {
  return (
    <header className={styles.header}>
      <Link to="/" className={styles.logo}>
        <span className={styles.star}>★</span>
        cute trading
        <span className={styles.star}>★</span>
      </Link>
      <p className={styles.sub}>polymarket predictions</p>
    </header>
  )
}
